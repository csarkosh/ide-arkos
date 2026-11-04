//! The sidebar's "watcher" (spec 6, ruling 1): every second, the expanded directories' and the
//! open viewers' fingerprints are read on the background executor; a changed directory is read
//! again, a changed file reloaded. Each video view is handed its file's fingerprint every
//! second, changed or not (`Root::tick_videos`). One stat per expanded directory per second. `git status`
//! runs a second after the last change it has not seen, and every ten seconds regardless; the
//! expanded worktrees' status, the worktree ages' redraw and the viewers' diffs, every ten seconds.
use crate::config_watch::fingerprint;
use crate::Root;
use gpui::{App, Entity};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const POLL: Duration = Duration::from_secs(1);
pub const STATUS_EVERY: Duration = Duration::from_secs(10);
const STATUS_DEBOUNCE: Duration = Duration::from_secs(1);

/// Whether to run `git status` now: never run yet, a change seen at least a second ago and not
/// yet reflected, or ten seconds since the last run (changes on every poll included).
pub fn status_due(last_run: Option<Instant>, changed_at: Option<Instant>, now: Instant) -> bool {
    match (last_run, changed_at) {
        (None, _) => true,
        (Some(run), Some(changed)) if changed > run => now.duration_since(changed) >= STATUS_DEBOUNCE || now.duration_since(run) >= STATUS_EVERY,
        (Some(run), _) => now.duration_since(run) >= STATUS_EVERY,
    }
}

/// Whether a `git status` result belongs to the newest run the window started.
pub fn status_result_applies(seq: u64, current: u64) -> bool {
    seq == current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusAction {
    Start,
    Defer,
}

/// The window's `git status` runs, one at a time so a slow repository neither piles up
/// processes nor drops every result as superseded: a request while one is in flight is
/// remembered, and however many came in, one more run starts when it lands.
#[derive(Debug, Default)]
pub struct StatusRuns {
    in_flight: bool,
    pending: bool,
}

impl StatusRuns {
    pub fn request(&mut self) -> StatusAction {
        if self.in_flight {
            self.pending = true;
            StatusAction::Defer
        } else {
            self.in_flight = true;
            StatusAction::Start
        }
    }

    /// A run landed: whether to request the one deferred while it ran.
    pub fn landed(&mut self) -> bool {
        self.in_flight = false;
        std::mem::take(&mut self.pending)
    }
}

/// What the ten-second tick runs besides `git status` (which `status_due` runs every ten
/// seconds regardless, so a `git init` is noticed), the expanded worktrees' changes and the
/// worktree ages (which run whether or not the open folder is a repository, since the folder may
/// hold many).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlowTick {
    /// The open viewers' diffs against HEAD.
    pub rediff: bool,
}

/// Outside a repository there is no HEAD to diff against: the tick diffs nothing until a status
/// finds a repository and clears `not_git`.
pub fn slow_tick_plan(not_git: bool) -> SlowTick {
    SlowTick { rediff: !not_git }
}

/// Whether the Worktrees section's safety-net pass is due: `FULL_PASS_EVERY` since the last one
/// started at `last`.
pub fn full_pass_due(last: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last) >= crate::wt_watch::FULL_PASS_EVERY
}

/// The paths whose fingerprint changed since `last`. A path not in `last` is a baseline.
pub fn changed(last: &HashMap<PathBuf, Option<(u128, u64)>>, now: &HashMap<PathBuf, Option<(u128, u64)>>) -> Vec<PathBuf> {
    now.iter().filter(|(p, f)| last.get(*p).is_some_and(|l| l != *f)).map(|(p, _)| p.clone()).collect()
}

pub fn start(root: Entity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last: HashMap<PathBuf, Option<(u128, u64)>> = HashMap::new();
        // The window ran the first `git status` as it opened, just before this started.
        let mut last_run: Option<Instant> = Some(Instant::now());
        let mut changed_at: Option<Instant> = None;
        let mut slow_run = Instant::now();
        loop {
            cx.background_executor().timer(POLL).await;
            let Ok(paths) = root.update(cx, |root, cx| root.watched_paths(cx)) else { return };
            let now = cx
                .background_executor()
                .spawn(async move { paths.into_iter().map(|p| (p.clone(), fingerprint(&p))).collect::<HashMap<_, _>>() })
                .await;
            let changed = changed(&last, &now);
            // Every video view hears every poll, not only a change: its follow rule reloads a file
            // once it has held still for one poll (spec 4).
            if root.update(cx, |root, cx| root.tick_videos(&now, cx)).is_err() {
                return;
            }
            // Merged, not replaced: a directory collapsed while it changed keeps its old
            // fingerprint, so expanding it again reads it again rather than showing stale rows.
            last.extend(now);
            if !changed.is_empty() {
                changed_at = Some(Instant::now());
                if root.update(cx, |root, cx| root.paths_changed(&changed, cx)).is_err() {
                    return;
                }
            }
            if status_due(last_run, changed_at, Instant::now()) {
                last_run = Some(Instant::now());
                if root.update(cx, |root, cx| root.refresh_status(cx)).is_err() {
                    return;
                }
            }
            // The expanded worktrees' changes, the worktree ages and the viewers' diffs, on the
            // slow timer: the changes are marked stale and reloaded, their rows staying up until
            // each lands. The worktree list itself follows FSEvents (`Root::worktree_event`), with
            // the ten minute pass run from `tick_worktree_ages`.
            if slow_run.elapsed() >= STATUS_EVERY {
                slow_run = Instant::now();
                let refreshed = root.update(cx, |root, cx| {
                    let plan = slow_tick_plan(root.explorer.not_git);
                    root.worktrees.mark_all_stale();
                    root.load_pending_worktrees(cx);
                    root.tick_worktree_ages(cx);
                    // A commit or checkout changes HEAD with the files untouched: the viewers'
                    // diffs are computed again.
                    if plan.rediff {
                        root.rediff_viewers(cx);
                    }
                });
                if refreshed.is_err() {
                    return;
                }
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_seen_before_with_a_different_fingerprint_count_as_changed() {
        let mut last = HashMap::new();
        last.insert(PathBuf::from("/a"), Some((1, 1)));
        last.insert(PathBuf::from("/b"), Some((2, 2)));
        last.insert(PathBuf::from("/gone"), Some((3, 3)));
        let mut now = HashMap::new();
        now.insert(PathBuf::from("/a"), Some((1, 1)));
        now.insert(PathBuf::from("/b"), Some((9, 2)));
        now.insert(PathBuf::from("/new"), Some((4, 4)));
        now.insert(PathBuf::from("/deleted"), None);
        last.insert(PathBuf::from("/deleted"), Some((5, 5)));
        let mut c = changed(&last, &now);
        c.sort();
        assert_eq!(c, [PathBuf::from("/b"), PathBuf::from("/deleted")], "a new path is a baseline, not a change; a deleted one is a change");
    }

    #[test]
    fn the_status_refresh_is_due_a_second_after_a_change_or_every_ten_seconds() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!status_due(Some(t0), None, t0 + Duration::from_millis(500)), "no change, not ten seconds yet");
        assert!(status_due(Some(t0), Some(t0 + Duration::from_millis(200)), t0 + Duration::from_millis(1300)), "a change over a second ago");
        assert!(!status_due(Some(t0), Some(t0 + Duration::from_millis(900)), t0 + Duration::from_millis(1300)), "a change under a second ago waits");
        assert!(status_due(Some(t0), None, t0 + STATUS_EVERY + Duration::from_millis(1)), "the slow timer");
        assert!(status_due(None, None, t0), "never run: run now");
        let fresh = t0 + STATUS_EVERY + Duration::from_millis(100);
        assert!(status_due(Some(t0), Some(fresh), fresh + Duration::from_millis(100)), "changes on every poll do not starve the slow timer");
    }

    #[test]
    fn status_runs_one_at_a_time_and_requests_meanwhile_collapse_into_one_more() {
        let mut runs = StatusRuns::default();
        assert_eq!(runs.request(), StatusAction::Start);
        assert_eq!(runs.request(), StatusAction::Defer, "one is in flight");
        assert_eq!(runs.request(), StatusAction::Defer);
        assert!(runs.landed(), "the requests made meanwhile run once more");
        assert_eq!(runs.request(), StatusAction::Start, "that run starts");
        assert!(!runs.landed(), "nothing came in while it ran");
        assert_eq!(runs.request(), StatusAction::Start);
    }

    #[test]
    fn outside_a_repository_the_slow_tick_diffs_nothing() {
        assert_eq!(slow_tick_plan(false), SlowTick { rediff: true });
        assert_eq!(slow_tick_plan(true), SlowTick { rediff: false }, "status alone keeps running, on its own timer; the worktrees follow FSEvents");
    }

    #[test]
    fn the_full_pass_runs_every_ten_minutes() {
        let t0 = Instant::now();
        assert!(!full_pass_due(t0, t0), "just ran");
        assert!(!full_pass_due(t0, t0 + Duration::from_secs(599)), "not ten minutes yet");
        assert!(full_pass_due(t0, t0 + Duration::from_secs(600)), "on the ten minute mark");
        assert!(full_pass_due(t0, t0 + Duration::from_secs(3_600)), "and long after");
        assert!(!full_pass_due(t0 + Duration::from_secs(5), t0), "a clock read before the pass is never due");
    }

    #[test]
    fn a_stale_status_result_is_dropped() {
        // The window bumps `status_seq` per run and applies a result only when it is the newest.
        assert!(status_result_applies(3, 3));
        assert!(!status_result_applies(2, 3), "an older run landing after a newer one started");
        assert!(!status_result_applies(4, 3), "a run the window does not know is ignored too");
    }
}
