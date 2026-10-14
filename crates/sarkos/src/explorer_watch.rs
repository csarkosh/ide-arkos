//! The sidebar's "watcher" (spec 6, ruling 1): every second, the expanded directories' and the
//! open viewers' fingerprints are read on the background executor; a changed directory is read
//! again, a changed file reloaded. One stat per expanded directory per second.
use crate::config_watch::fingerprint;
use crate::Root;
use gpui::{App, Entity};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const POLL: Duration = Duration::from_secs(1);

/// The paths whose fingerprint changed since `last`. A path not in `last` is a baseline.
pub fn changed(last: &HashMap<PathBuf, Option<(u128, u64)>>, now: &HashMap<PathBuf, Option<(u128, u64)>>) -> Vec<PathBuf> {
    now.iter().filter(|(p, f)| last.get(*p).is_some_and(|l| l != *f)).map(|(p, _)| p.clone()).collect()
}

pub fn start(root: Entity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last: HashMap<PathBuf, Option<(u128, u64)>> = HashMap::new();
        loop {
            cx.background_executor().timer(POLL).await;
            let Ok(paths) = root.update(cx, |root, cx| root.watched_paths(cx)) else { return };
            let now = cx
                .background_executor()
                .spawn(async move { paths.into_iter().map(|p| (p.clone(), fingerprint(&p))).collect::<HashMap<_, _>>() })
                .await;
            let changed = changed(&last, &now);
            // Merged, not replaced: a directory collapsed while it changed keeps its old
            // fingerprint, so expanding it again reads it again rather than showing stale rows.
            last.extend(now);
            if changed.is_empty() {
                continue;
            }
            if root.update(cx, |root, cx| root.paths_changed(&changed, cx)).is_err() {
                return;
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
}
