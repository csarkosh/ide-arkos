//! Notices edits to the config file (spec 7: "watched and applied live"). Polls the file's
//! mtime and size twice a second from the background executor: one stat per half second, no
//! watcher thread, no new dependency. A deleted file counts as a change and is recreated with
//! the defaults by `config::load`.
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

/// Polls until the window is gone; every change asks `Root::reload_config`.
pub fn start(root: Entity<Root>, path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last = fingerprint(&path);
        loop {
            cx.background_executor().timer(POLL).await;
            let p = path.clone();
            let now = cx.background_executor().spawn(async move { fingerprint(&p) }).await;
            if now == last {
                continue;
            }
            last = now;
            if root.update(cx, |root, cx| root.reload_config(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

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
