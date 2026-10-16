//! Video and audio in the rendered view (spec 5). An image whose path is a local video (`mp4`,
//! `mov`, `m4v`) is drawn as a poster that Quick Look's thumbnailer (`qlmanage`, part of macOS)
//! makes once per file and version, cached at `<cache>/posters/<sha256 of the path and its
//! mtime>.png`; local audio (`mp3`, `m4a`, `wav`, `aac`) is a one-line box. Cmd+click opens a video
//! in QuickTime Player and audio with the system. A `webm` (which neither Quick Look nor QuickTime
//! takes) and remote video or audio are links instead, and nothing is fetched. One poster is made
//! at a time in the process (a window is a process), through the `queue()` gate; the folder is
//! swept at launch (`cache::sweep_all`).

use crate::remote_image::{self, FetchQueue};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

/// The extensions of the videos drawn as posters, and of the audio drawn as a box.
const VIDEO: [&str; 3] = ["mp4", "mov", "m4v"];
const AUDIO: [&str; 4] = ["mp3", "m4a", "wav", "aac"];
/// Video that is only ever a link: Quick Look cannot make its poster and QuickTime cannot play it.
const LINK_ONLY: &str = "webm";
/// The longest side of a poster, in pixels, as Quick Look is asked for it.
const POSTER_SIZE: &str = "1024";
/// The wall clock after which a `qlmanage` that is still running is killed.
const BACKSTOP: Duration = Duration::from_secs(20);

/// What a media path is drawn as: a video's poster or an audio file's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    Video,
    Audio,
}

/// Where one video's poster is, for the rendered view: being made (or waiting for its turn),
/// in the cache at this path, or not to be had (the file is missing, or Quick Look made none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poster {
    Making,
    Ready(PathBuf),
    Failed,
}

/// The extension of `path`'s last part, lowercased.
fn extension(path: &str) -> Option<String> {
    Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
}

/// What a local image path is drawn as when its extension (in any case) is video or audio; `None`
/// for anything else, for a `webm`, and for a web address.
pub fn kind(path: &str) -> Option<Media> {
    if remote_image::is_remote(path) {
        return None;
    }
    let ext = extension(path)?;
    if VIDEO.contains(&ext.as_str()) {
        Some(Media::Video)
    } else if AUDIO.contains(&ext.as_str()) {
        Some(Media::Audio)
    } else {
        None
    }
}

/// Whether an image path is media drawn as a link: a web address whose path (before any query or
/// fragment) ends in a video, audio or `webm` extension, or a local `webm`.
pub fn is_link(path: &str) -> bool {
    if remote_image::is_remote(path) {
        let bare = path.split(['?', '#']).next().unwrap_or(path);
        return extension(bare).is_some_and(|e| e == LINK_ONLY || VIDEO.contains(&e.as_str()) || AUDIO.contains(&e.as_str()));
    }
    extension(path).is_some_and(|e| e == LINK_ONLY)
}

/// The folder posters are kept in, made by the first poster.
pub fn poster_dir() -> PathBuf {
    crate::cache::root().join("posters")
}

/// Where the poster of `file` as it was at `mtime_secs` goes in `dir`: the SHA-256 in hex of the
/// path and the time, so a changed file gets a new poster.
pub fn poster_path(dir: &Path, file: &Path, mtime_secs: u64) -> PathBuf {
    let mut hash = Sha256::new();
    hash.update(file.as_os_str().as_encoded_bytes());
    hash.update(b"\0");
    hash.update(mtime_secs.to_string().as_bytes());
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    dir.join(hex).with_extension("png")
}

/// `file`'s modification time in whole seconds since 1970, or `None` when it cannot be read.
pub fn mtime_secs(file: &Path) -> Option<u64> {
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// `qlmanage`'s arguments for a thumbnail of `file` at most `POSTER_SIZE` pixels on its longest
/// side, written into `out_dir` (as `<file name>.png`). Both paths are absolute, so neither can
/// be taken for an option.
pub fn qlmanage_args(file: &Path, out_dir: &Path) -> Vec<String> {
    vec!["-t".into(), "-s".into(), POSTER_SIZE.into(), "-o".into(), out_dir.to_string_lossy().into_owned(), file.to_string_lossy().into_owned()]
}

/// `/usr/bin/open`'s arguments for playing a video in QuickTime Player: always named, never a
/// bare `open`.
pub fn quicktime_args(file: &Path) -> Vec<String> {
    vec!["-a".into(), "QuickTime Player".into(), file.to_string_lossy().into_owned()]
}

/// `/usr/bin/open`'s arguments for opening a media file with the app the system has for its
/// type. Only a path `kind` or `is_link` admitted (an audio or `webm` extension) is opened this
/// way, so it is never a program, and it is absolute, so it is never taken for an option.
pub fn system_args(file: &Path) -> Vec<String> {
    vec![file.to_string_lossy().into_owned()]
}

/// A file size as the audio box shows it: bytes below a KB, else KB, MB or GB (of 1024) with one
/// decimal.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// The thumbnail Quick Look wrote into a job's folder: the one `.png` there. `qlmanage` can exit
/// with success and write nothing, so no file (or more than one) means it failed.
pub fn thumbnail_in(dir: &Path) -> Option<PathBuf> {
    let mut pngs = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png") && p.is_file());
    let png = pngs.next()?;
    pngs.next().is_none().then_some(png)
}

/// The process's poster jobs (video files), one at a time.
static THUMBNAILS: Mutex<FetchQueue<PathBuf>> = Mutex::new(FetchQueue::new(1));

/// The process-wide poster queue, locked. A panic while it was held leaves it as it was.
pub fn queue() -> MutexGuard<'static, FetchQueue<PathBuf>> {
    THUMBNAILS.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the poster of a video the rendered view shows becomes, `None` when it stays as it is:
/// one not seen yet (`state` is `None`) is ready when an earlier run left it in the cache, failed
/// when the file cannot be read, else queued and being made; one being made whose job has ended
/// (the queue no longer has it) is ready or failed by what the cache holds. `dir` gives the
/// cache's folder and is asked for only when it is looked in.
pub fn advance(state: Option<&Poster>, file: &Path, dir: impl FnOnce() -> PathBuf) -> Option<Poster> {
    let made = |dir: PathBuf| mtime_secs(file).map(|m| poster_path(&dir, file, m)).filter(|p| p.is_file());
    match state {
        None => Some(match mtime_secs(file) {
            None => Poster::Failed,
            Some(m) => match poster_path(&dir(), file, m) {
                poster if poster.is_file() => Poster::Ready(poster),
                _ => {
                    queue().want(file.to_path_buf());
                    Poster::Making
                }
            },
        }),
        Some(Poster::Making) if !queue().is_pending(file) => Some(made(dir()).map_or(Poster::Failed, Poster::Ready)),
        Some(_) => None,
    }
}

/// Makes `file`'s poster in `dir` (made when missing) with `/usr/bin/qlmanage`, blocking the
/// calling thread: into a folder of this job's own (`job.<pid>`, as two windows may make posters
/// at once), then the one thumbnail there renamed to `poster_path`. The job's folder is removed
/// either way. Returns where the poster landed.
pub fn make_poster(dir: &Path, file: &Path) -> Result<PathBuf, String> {
    let mtime = mtime_secs(file).ok_or_else(|| format!("cannot read {}", file.display()))?;
    let poster = poster_path(dir, file, mtime);
    let job = dir.join(format!("job.{}", std::process::id()));
    // A folder left by an earlier process with this pid that ended in the middle of a job.
    let _ = std::fs::remove_dir_all(&job);
    std::fs::create_dir_all(&job).map_err(|e| format!("cannot make {}: {e}", job.display()))?;
    let made = run_qlmanage(file, &job)
        .and_then(|()| thumbnail_in(&job).ok_or_else(|| "Quick Look made no thumbnail".to_string()))
        .and_then(|png| std::fs::rename(&png, &poster).map_err(|e| e.to_string()))
        .map(|()| poster);
    let _ = std::fs::remove_dir_all(&job);
    made
}

/// Runs qlmanage for `file` into `job` with a minimal environment, killed (with its process
/// group) once `BACKSTOP` passes.
fn run_qlmanage(file: &Path, job: &Path) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("/usr/bin/qlmanage");
    cmd.args(qlmanage_args(file, job)).env_clear().env("PATH", "/usr/bin:/bin");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    // qlmanage reports on stdout and stderr, neither of which is read: the folder decides.
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().map_err(|e| format!("cannot run /usr/bin/qlmanage: {e}"))?;
    let status = remote_image::wait_bounded(&mut child, BACKSTOP, "qlmanage")?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("qlmanage: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn media_is_told_by_extension_and_the_poster_and_open_arguments_are_fixed() {
        assert_eq!(kind("clip.MOV"), Some(Media::Video));
        assert_eq!(kind("a/b.m4v"), Some(Media::Video));
        assert_eq!(kind("a.webm"), None, "webm is a link");
        assert_eq!(kind("song.m4a"), Some(Media::Audio));
        assert_eq!(kind("a.png"), None);
        assert_eq!(kind("https://x/a.mp4"), None, "remote media is a link");
        let p = poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_000);
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
        assert_ne!(p, poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_001), "the mtime is in the key");
        assert_eq!(qlmanage_args(Path::new("/v/clip.mov"), Path::new("/c/job")), ["-t", "-s", "1024", "-o", "/c/job", "/v/clip.mov"]);
        assert_eq!(quicktime_args(Path::new("/v/clip.mov")), ["-a", "QuickTime Player", "/v/clip.mov"]);
        assert_eq!(size_text(950), "950 B"); assert_eq!(size_text(2_500), "2.4 KB"); assert_eq!(size_text(5_300_000), "5.1 MB");
        let dir = tempfile::tempdir().unwrap();
        assert!(thumbnail_in(dir.path()).is_none(), "no png: failed");
        std::fs::write(dir.path().join("clip.mov.png"), "x").unwrap();
        assert_eq!(thumbnail_in(dir.path()), Some(dir.path().join("clip.mov.png")));
    }

    #[test]
    fn remote_media_and_webm_are_links_and_audio_opens_with_the_system() {
        assert!(is_link("https://x/a.MP4?t=3") && is_link("http://x/s.mp3#a") && is_link("https://x/a.webm") && is_link("clips/a.webm"));
        assert!(!is_link("clip.mov") && !is_link("https://x/a.png") && !is_link("https://x.com"));
        assert_eq!(system_args(Path::new("/m/song.mp3")), ["/m/song.mp3"]);
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024), "3.0 GB");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), "x").unwrap();
        std::fs::write(dir.path().join("b.png"), "x").unwrap();
        assert!(thumbnail_in(dir.path()).is_none(), "two pngs: not the one Quick Look made");
    }

    #[test]
    fn a_poster_is_ready_from_the_cache_or_made_until_its_job_ends() {
        let dir = tempfile::tempdir().unwrap();
        let at = || dir.path().to_path_buf();
        let (old, new) = (dir.path().join("old.mov"), dir.path().join("new.mp4"));
        std::fs::write(&old, "v").unwrap();
        std::fs::write(&new, "v").unwrap();
        let cached = poster_path(dir.path(), &old, mtime_secs(&old).unwrap());
        std::fs::write(&cached, "png").unwrap();
        assert_eq!(advance(None, &old, at), Some(Poster::Ready(cached)), "a poster an earlier run left is used at once");
        assert!(!queue().is_pending(old.as_path()));
        assert_eq!(advance(None, &dir.path().join("gone.mov"), at), Some(Poster::Failed), "a missing file has no poster");
        assert_eq!(advance(None, &new, at), Some(Poster::Making));
        assert!(queue().is_pending(new.as_path()), "a video with no poster is queued");
        assert_eq!(advance(Some(&Poster::Making), &new, at), None, "it is being made while queued");
        queue().done(new.as_path());
        assert_eq!(advance(Some(&Poster::Making), &new, at), Some(Poster::Failed), "a job that left nothing failed");
        let made = poster_path(dir.path(), &new, mtime_secs(&new).unwrap());
        std::fs::write(&made, "png").unwrap();
        assert_eq!(advance(Some(&Poster::Making), &new, at), Some(Poster::Ready(made)));
        assert_eq!(advance(Some(&Poster::Failed), &new, at), None, "a failed poster is not made again in this view");
    }
}
