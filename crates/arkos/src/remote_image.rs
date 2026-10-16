//! Remote images (spec 4): an image whose path is an `http(s)` url is fetched through the system
//! curl (TLS and proxies are macOS's business, as for the updater) into
//! `<cache>/images/<sha256 of the url>`, renamed with the extension its first bytes show, so
//! `open -a Preview` knows its type. At most `MAX_IN_FLIGHT` fetches run at once in the process
//! (a window is a process), through the `queue()` gate; a url is fetched once while it is
//! wanted, and a failed one again only when its file is opened again. The cache outlives the
//! session and is swept at launch (`cache::sweep_all`).

use sha2::{Digest, Sha256};
use std::borrow::Borrow;
use std::collections::{BTreeSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The most bytes a fetched image may have.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// The most seconds curl itself spends on one fetch.
const MAX_SECS: u64 = 15;
/// The wall clock after which a curl that is still running is killed, should it hang past its
/// own `--max-time`.
const BACKSTOP: Duration = Duration::from_secs(20);
/// The most fetches in flight in the process.
pub const MAX_IN_FLIGHT: usize = 4;
/// The extensions a fetched file is given, from `sniff_extension`.
const EXTENSIONS: [&str; 5] = ["png", "jpg", "gif", "webp", "svg"];
/// How many of a fetched file's first bytes `sniff_extension` looks at: enough for an SVG's
/// leading white space.
const SNIFF_BYTES: u64 = 512;

/// Where one remote image is, for the rendered view: being fetched (or waiting for a slot),
/// in the cache at this path, or not to be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    Fetching,
    Ready(PathBuf),
    Failed,
}

/// Whether an image's path is a web address: `http://` or `https://`, in any case.
pub fn is_remote(path: &str) -> bool {
    let lower = path.get(..8).unwrap_or(path).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The folder fetched images are kept in, made by the first fetch.
pub fn cache_dir() -> PathBuf {
    crate::cache::root().join("images")
}

/// Where `url`'s image goes in `dir` before its type is known: the SHA-256 of the url in hex,
/// with no extension (a fetch adds the sniffed one).
pub fn cache_path(dir: &Path, url: &str) -> PathBuf {
    let hex: String = Sha256::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    dir.join(hex)
}

/// `url`'s image in `dir` when an earlier fetch (in this process or another) left it there,
/// under whichever extension it was given, and it is not near its sweep (`cache::fresh`).
pub fn cached(dir: &Path, url: &str) -> Option<PathBuf> {
    let base = cache_path(dir, url);
    EXTENSIONS.iter().map(|ext| base.with_extension(ext)).find(|p| crate::cache::fresh(p, crate::cache::IMAGES_AGE))
}

/// curl's arguments for fetching `url` into `dest`: `--disable` first, so no `~/.curlrc` adds
/// cookies, a referer or anything else; http and https only, redirects included, at most five of
/// them; `MAX_SECS` seconds and `MAX_BYTES` bytes (a server that does not say the size is
/// checked again after the fetch).
pub fn curl_args(url: &str, dest: &Path) -> Vec<String> {
    vec![
        "--disable".into(), "--fail".into(), "--silent".into(), "--show-error".into(), "--location".into(),
        "--max-redirs".into(), "5".into(),
        "--proto".into(), "=http,https".into(),
        "--proto-redir".into(), "=http,https".into(),
        "--max-time".into(), MAX_SECS.to_string(),
        "--max-filesize".into(), MAX_BYTES.to_string(),
        "--output".into(), dest.to_string_lossy().into_owned(),
        "--".into(), url.to_string(),
    ]
}

/// The extension for an image from its first bytes (`SNIFF_BYTES` of them): PNG, JPEG, GIF and
/// WebP by their signatures, SVG when `<svg` or `<?xml` follows an optional byte order mark and
/// white space. Anything else (an HTML page a captive portal or a rate limit served with a 200)
/// is `None`, not an image.
pub fn sniff_extension(head: &[u8]) -> Option<&'static str> {
    let text = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let text = &text[text.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(text.len())..];
    if head.starts_with(b"\x89PNG") {
        Some("png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if head.starts_with(b"GIF8") {
        Some("gif")
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if text.starts_with(b"<svg") || text.starts_with(b"<?xml") {
        Some("svg")
    } else {
        None
    }
}

/// `/usr/bin/open`'s arguments for showing an image in Preview: always named, never a bare
/// `open`, which could run a program.
pub fn preview_args(file: &Path) -> Vec<String> {
    vec!["-a".into(), "Preview".into(), file.to_string_lossy().into_owned()]
}

/// The extensions of the image files Preview is asked to show when their first bytes are not
/// one `sniff_extension` knows (a TIFF, a HEIC, a BMP).
const PREVIEW_EXTENSIONS: [&str; 11] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "tif", "tiff", "heic", "bmp", "ico"];

/// The file to show in Preview for the image at `file`: its canonical path, when `file` is
/// absolute and resolves (symlinks followed) to a plain file whose first bytes are an image's
/// (`sniff_extension`) or whose resolved name has an image's extension; else `None`, so a link
/// named like an image that leads to an app, a folder or a script is refused. It reads the disk,
/// so the caller runs it on the background executor, and the canonical path is what is opened.
pub fn checked_preview_path(file: &Path) -> Option<PathBuf> {
    if !file.is_absolute() {
        return None;
    }
    let canonical = std::fs::canonicalize(file).ok().filter(|p| p.is_file())?;
    let mut head = Vec::with_capacity(SNIFF_BYTES as usize);
    let sniffed = std::fs::File::open(&canonical).and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut head)).is_ok() && sniff_extension(&head).is_some();
    let named = canonical.extension().and_then(|e| e.to_str()).is_some_and(|e| PREVIEW_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    (sniffed || named).then_some(canonical)
}

/// The jobs to run (urls to fetch here, video files to make posters of in `media`): each is in
/// the queue once from its `want` to its `done`, waiting in order until a slot is free, then in
/// flight, at most `limit` at once.
#[derive(Debug)]
pub struct FetchQueue<T = String> {
    limit: usize,
    seen: BTreeSet<T>,
    waiting: VecDeque<T>,
    flying: BTreeSet<T>,
}

impl<T: Ord + Clone> FetchQueue<T> {
    /// An empty queue that runs at most `limit` jobs at once.
    pub const fn new(limit: usize) -> FetchQueue<T> {
        FetchQueue { limit, seen: BTreeSet::new(), waiting: VecDeque::new(), flying: BTreeSet::new() }
    }

    /// Queues `job`, unless it is already waiting or in flight.
    pub fn want(&mut self, job: T) {
        if self.seen.insert(job.clone()) {
            self.waiting.push_back(job);
        }
    }

    /// The waiting jobs that now have a slot, moved to in flight: the caller runs them.
    pub fn start_ready(&mut self) -> Vec<T> {
        let free = self.limit.saturating_sub(self.in_flight()).min(self.waiting.len());
        let ready: Vec<T> = self.waiting.drain(..free).collect();
        self.flying.extend(ready.iter().cloned());
        ready
    }

    /// How many jobs are running.
    pub fn in_flight(&self) -> usize {
        self.flying.len()
    }

    /// `job` ended, either way: its slot is free, and wanting it again runs it again.
    pub fn done<Q: Ord + ?Sized>(&mut self, job: &Q)
    where
        T: Borrow<Q>,
    {
        self.flying.remove(job);
        self.seen.remove(job);
    }

    /// Whether `job` is waiting or in flight.
    pub fn is_pending<Q: Ord + ?Sized>(&self, job: &Q) -> bool
    where
        T: Borrow<Q>,
    {
        self.seen.contains(job)
    }
}

/// The process's fetches, shared by every viewer in the window.
static FETCHES: Mutex<FetchQueue> = Mutex::new(FetchQueue::new(MAX_IN_FLIGHT));

/// The process-wide fetch queue, locked. A panic while it was held leaves it as it was.
pub fn queue() -> MutexGuard<'static, FetchQueue> {
    FETCHES.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a web image the rendered view shows becomes, `None` when it stays as it is: one not seen
/// yet (`state` is `None`) is ready when an earlier fetch left it in the cache, else queued and
/// fetching; one fetching whose fetch has ended (the queue no longer has it) is ready or failed by
/// what the cache holds. `dir` gives the cache's folder and is asked for only when it is looked
/// in.
pub fn advance(state: Option<&Remote>, url: &str, dir: impl FnOnce() -> PathBuf) -> Option<Remote> {
    match state {
        None => Some(match cached(&dir(), url) {
            Some(file) => Remote::Ready(file),
            None => {
                queue().want(url.to_string());
                Remote::Fetching
            }
        }),
        Some(Remote::Fetching) if !queue().is_pending(url) => Some(cached(&dir(), url).map_or(Remote::Failed, Remote::Ready)),
        Some(_) => None,
    }
}

/// Fetches `url` into `dir` (made when missing) with `/usr/bin/curl`, blocking the calling
/// thread: into `<cache_path>.<pid>.part` first (two windows may fetch the same url), then, when
/// it is not empty and at most `MAX_BYTES`, renamed with its sniffed extension. Returns where it
/// landed; on any failure nothing is left behind.
pub fn fetch(dir: &Path, url: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let dest = cache_path(dir, url);
    let part = dest.with_extension(format!("{}.part", std::process::id()));
    let landed = run_curl(url, &part).and_then(|()| land(&part, &dest));
    if landed.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    landed
}

/// Runs curl for `url` into `part` with a minimal environment, killed (with its process group)
/// once `BACKSTOP` passes, as the updater's `run_bounded` does.
fn run_curl(url: &str, part: &Path) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(curl_args(url, part)).env_clear().env("PATH", "/usr/bin:/bin");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    // stderr is one line at most (`--silent --show-error`), so the pipe cannot fill while the
    // loop below waits.
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).process_group(0).spawn().map_err(|e| format!("cannot run /usr/bin/curl: {e}"))?;
    let status = wait_bounded(&mut child, BACKSTOP, "curl")?;
    if status.success() {
        return Ok(());
    }
    let mut err = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut err);
    }
    Err(format!("curl: {} ({status})", err.trim()))
}

/// Waits for `child`, spawned in a process group of its own (`process_group(0)`), until it exits
/// or `backstop` passes; then the group is killed and the wait is an error naming `name`.
pub fn wait_bounded(child: &mut Child, backstop: Duration, name: &str) -> Result<ExitStatus, String> {
    let group = child.id() as libc::pid_t;
    let deadline = Instant::now() + backstop;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            waited => {
                // SAFETY: killpg only signals; the group is the child's own, made at spawn.
                unsafe { libc::killpg(group, libc::SIGKILL) };
                let _ = child.wait();
                return Err(match waited {
                    Err(e) => e.to_string(),
                    _ => format!("{name} timed out after {} s", backstop.as_secs()),
                });
            }
        }
    }
}

/// A finished download: refused when empty, over `MAX_BYTES` or not an image by its first bytes
/// (`sniff_extension`), and the part file removed; else renamed from `part` to `dest` with the
/// extension those bytes show.
fn land(part: &Path, dest: &Path) -> Result<PathBuf, String> {
    let landed = (|| {
        let len = std::fs::metadata(part).map_err(|e| e.to_string())?.len();
        if len == 0 || len > MAX_BYTES {
            return Err(format!("{len} bytes, not an image of at most {MAX_BYTES} bytes"));
        }
        let mut head = Vec::with_capacity(SNIFF_BYTES as usize);
        std::fs::File::open(part).and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut head)).map_err(|e| e.to_string())?;
        let ext = sniff_extension(&head).ok_or_else(|| "not an image (PNG, JPEG, GIF, WebP or SVG)".to_string())?;
        let to = dest.with_extension(ext);
        std::fs::rename(part, &to).map_err(|e| e.to_string())?;
        Ok(to)
    })();
    if landed.is_err() {
        let _ = std::fs::remove_file(part);
    }
    landed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn remote_urls_get_a_cache_path_and_curl_arguments_with_the_limits() {
        assert!(is_remote("HTTPS://x.y/a.png") && is_remote("http://x/a") && !is_remote("img/a.png") && !is_remote("file:///a.png"));
        let p = cache_path(Path::new("/c"), "https://x.y/a.PNG?x=1");
        assert_eq!(p.file_name().unwrap().len(), 64, "the sha256 hex, no extension until sniffed");
        assert_ne!(cache_path(Path::new("/c"), "https://x.y/a"), cache_path(Path::new("/c"), "https://x.y/b"));
        let a = curl_args("https://x.y/a.png", Path::new("/c/dest.1.part"));
        assert_eq!(a, ["--disable", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "5", "--proto", "=http,https", "--proto-redir", "=http,https", "--max-time", "15", "--max-filesize", "10485760", "--output", "/c/dest.1.part", "--", "https://x.y/a.png"]);
        assert_eq!(sniff_extension(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(sniff_extension(b"\xff\xd8\xff\xe0"), Some("jpg"));
        assert_eq!(sniff_extension(b"GIF89a"), Some("gif"));
        assert_eq!(sniff_extension(b"RIFF....WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_extension(b"<svg xmlns"), Some("svg"));
        assert_eq!(sniff_extension(b"\xef\xbb\xbf\n  <?xml version"), Some("svg"), "a byte order mark and white space first");
        assert_eq!(sniff_extension(b"<!DOCTYPE html><html>"), None);
        assert_eq!(sniff_extension(b""), None);
        assert_eq!(preview_args(Path::new("/c/abc.png")), ["-a", "Preview", "/c/abc.png"]);
    }

    #[test]
    fn a_remote_image_is_ready_from_the_cache_or_fetching_until_its_fetch_ends() {
        let dir = tempfile::tempdir().unwrap();
        let at = || dir.path().to_path_buf();
        let (old, new) = ("https://cached.invalid/a.png", "https://fresh.invalid/b.png");
        let file = cache_path(dir.path(), old).with_extension("gif");
        std::fs::write(&file, "GIF89a").unwrap();
        assert_eq!(advance(None, old, at), Some(Remote::Ready(file)), "a file an earlier fetch left is used at once");
        assert!(!queue().is_pending(old));
        assert_eq!(advance(None, new, at), Some(Remote::Fetching));
        assert!(queue().is_pending(new), "an image not in the cache is queued");
        assert_eq!(advance(Some(&Remote::Fetching), new, at), None, "it stays fetching while queued");
        queue().done(new);
        assert_eq!(advance(Some(&Remote::Fetching), new, at), Some(Remote::Failed), "a fetch that left nothing failed");
        let landed = cache_path(dir.path(), new).with_extension("svg");
        std::fs::write(&landed, "<svg/>").unwrap();
        assert_eq!(advance(Some(&Remote::Fetching), new, at), Some(Remote::Ready(landed)));
        assert_eq!(advance(Some(&Remote::Failed), new, at), None, "a failed image is not fetched again in this view");
    }

    #[test]
    fn a_page_that_is_not_an_image_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://portal.invalid/badge.png";
        let dest = cache_path(dir.path(), url);
        let part = dest.with_extension("1.part");
        std::fs::write(&part, "<!DOCTYPE html>\n<html><body>Sign in to the Wi-Fi</body></html>\n").unwrap();
        assert!(land(&part, &dest).is_err());
        assert!(!part.exists(), "the part file is removed");
        assert_eq!(cached(dir.path(), url), None, "nothing is cached under any extension");
        queue().want(url.to_string());
        queue().done(url);
        assert_eq!(advance(Some(&Remote::Fetching), url, || dir.path().to_path_buf()), Some(Remote::Failed));
        std::fs::write(&part, "\u{feff}\n  <svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        assert_eq!(land(&part, &dest), Ok(dest.with_extension("svg")), "an SVG after a byte order mark and white space lands");
    }

    #[test]
    fn only_a_plain_image_file_is_shown_in_preview() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        // The canonical temp folder: on macOS `/var` is a link to `/private/var`.
        let d = std::fs::canonicalize(dir.path()).unwrap();
        let (png, tiff, page, script) = (d.join("a.png"), d.join("scan.tiff"), d.join("notes.txt"), d.join("run.sh"));
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(&tiff, b"II*\0").unwrap();
        std::fs::write(&page, "plain text").unwrap();
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::create_dir(d.join("folder.png")).unwrap();
        symlink(&script, d.join("evil.png")).unwrap();
        symlink(&png, d.join("pic.dat")).unwrap();
        assert_eq!(checked_preview_path(&png), Some(png.clone()));
        assert_eq!(checked_preview_path(&tiff), Some(tiff.clone()), "an image's extension is enough");
        assert_eq!(checked_preview_path(&d.join("pic.dat")), Some(png.clone()), "a link to an image: its target is opened");
        assert_eq!(checked_preview_path(&d.join("evil.png")), None, "a link named like an image that leads to a script");
        assert_eq!(checked_preview_path(&d.join("folder.png")), None, "a folder");
        assert_eq!(checked_preview_path(&page), None, "not an image");
        assert_eq!(checked_preview_path(&d.join("gone.png")), None, "missing");
        assert_eq!(checked_preview_path(Path::new("a.png")), None, "not absolute");
    }

    #[test]
    fn an_image_near_its_sweep_age_is_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://old.invalid/a.png";
        let file = cache_path(dir.path(), url).with_extension("png");
        std::fs::write(&file, b"\x89PNG").unwrap();
        assert_eq!(cached(dir.path(), url), Some(file.clone()));
        let old = std::time::SystemTime::now() - (crate::cache::IMAGES_AGE - Duration::from_secs(3600));
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
        assert_eq!(cached(dir.path(), url), None, "within a day of the sweep: missing, so it is fetched again");
    }

    #[test]
    fn the_fetch_queue_holds_four_in_flight_and_never_repeats_a_url() {
        let mut q = FetchQueue::new(MAX_IN_FLIGHT);
        for i in 0..6 {
            q.want(format!("https://x/{i}"));
        }
        q.want("https://x/0".into());
        assert_eq!(q.start_ready().len(), 4);
        assert_eq!(q.in_flight(), 4);
        q.done("https://x/0");
        assert_eq!(q.start_ready(), vec!["https://x/4".to_string()]);
        assert!(q.start_ready().is_empty());
    }
}
