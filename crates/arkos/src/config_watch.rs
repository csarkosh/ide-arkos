//! Notices edits to the config file (spec 7: "watched and applied live") and to the configured
//! theme's file (milestone 8b). Polls both files' mtime and size twice a second from the
//! background executor: two stats per half second, no watcher thread, no new dependency. A
//! deleted config file counts as a change and is recreated with the defaults by `config::load`;
//! a theme file appearing or vanishing counts as a change too.
use crate::Root;
use gpui::{App, Entity};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub const POLL: Duration = Duration::from_millis(500);

/// (mtime in nanoseconds since the epoch, length), or `None` when the file is missing.
pub fn fingerprint(path: &Path) -> Option<(u128, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((mtime, meta.len()))
}

/// Whether the config file's or the theme file's fingerprint moved since the last poll.
pub fn changed(last: &[Option<(u128, u64)>; 2], now: &[Option<(u128, u64)>; 2]) -> bool {
    last != now
}

/// Polls until the window is gone; every change of `path` or of `Root::theme_file` asks
/// `Root::reload_config`. The theme path is asked for again after every poll: when it moves (a
/// reload named another theme, or macOS changed appearance), the new file's fingerprint is taken
/// fresh, so the move itself is not counted as an edit.
pub fn start(root: Entity<Root>, path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let Ok(mut theme_path) = root.read_with(cx, |root, _| root.theme_file()) else {
            return;
        };
        let mut last = [fingerprint(&path), fingerprint(&theme_path)];
        loop {
            cx.background_executor().timer(POLL).await;
            let (p, t) = (path.clone(), theme_path.clone());
            let now = cx.background_executor().spawn(async move { [fingerprint(&p), fingerprint(&t)] }).await;
            if changed(&last, &now) && root.update(cx, |root, cx| root.reload_config(cx)).is_err() {
                break;
            }
            last = now;
            let Ok(next) = root.read_with(cx, |root, _| root.theme_file()) else {
                break;
            };
            if next != theme_path {
                theme_path = next;
                let t = theme_path.clone();
                last[1] = cx.background_executor().spawn(async move { fingerprint(&t) }).await;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_path_changing_counts() {
        let a = Some((1, 1));
        let b = Some((2, 2));
        assert!(!changed(&[a, None], &[a, None]));
        assert!(changed(&[a, None], &[b, None]));
        assert!(changed(&[a, None], &[a, b]), "a theme file appearing");
        assert!(changed(&[a, b], &[a, None]), "a theme file deleted");
    }

    #[test]
    fn fingerprint_changes_with_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        assert_eq!(fingerprint(&file), None);
        std::fs::write(&file, "a").unwrap();
        let one = fingerprint(&file).expect("exists");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, "bb").unwrap();
        let two = fingerprint(&file).expect("exists");
        assert_ne!(one, two);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, "cc").unwrap();
        assert_ne!(fingerprint(&file), Some(two), "the same length at a later time is a change");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(fingerprint(&file), None);
    }
}
