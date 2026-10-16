//! Mermaid diagrams in the rendered view (spec 6): a fenced code block whose info string is
//! `mermaid` is drawn by Google Chrome, headless, from a page Arkos writes into
//! `<cache>/mermaid/` beside the bundled mermaid script, to a PNG cached by the SHA-256 of the
//! source, the theme's colours and the scale (`<key>.png`, with its size in `<key>.json`). Two
//! Chrome runs per render: `--dump-dom` reads the size the page puts in its title, then
//! `--screenshot` takes a transparent picture of exactly that size at twice the pixels. Nothing
//! leaves the machine: the page is a local file whose content security policy allows only inline
//! code and local files, and Chrome runs with every background service off, the name resolver
//! pointed nowhere and a throwaway profile of the process's own (`profile-<pid>`, made for each
//! render and removed after it). One render runs at a time in the process (a window is a
//! process), through the `queue()` gate, and only for a block drawn rendered, never for the one
//! being typed in; the folder is swept at launch (`cache::sweep_all`).

use crate::remote_image::{self, FetchQueue};
use crate::theme::Chrome;
use sha2::{Digest, Sha256};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// The version of the vendored mermaid, in a macro so `SCRIPT_NAME` can be built from it.
macro_rules! version {
    () => {
        "11.17.2"
    };
}

/// The vendored mermaid's version (`crates/arkos/assets/mermaid.min.js`, from
/// `scripts/vendor-mermaid.sh`).
pub const VERSION: &str = version!();
/// The name the bundled script is written out under, beside the pages: one file per version.
pub const SCRIPT_NAME: &str = concat!("mermaid-", version!(), ".min.js");
/// The bundled script.
const SCRIPT: &[u8] = include_bytes!("../assets/mermaid.min.js");
/// The device scale factor of the screenshot: a diagram's PNG has twice the pixels of its size
/// in CSS pixels, and is drawn at that size.
pub const SCALE: u32 = 2;
/// The widest a diagram is drawn, in CSS pixels; a wider one is scaled down to it.
const MAX_W: u32 = 2000;
/// The longest side, in pixels, of an image gpui can draw (its Metal atlas's limit): a PNG with
/// a longer side would be ready and draw nothing.
const MAX_SIDE: u32 = 16_384;
/// The tallest diagram that is rendered, in CSS pixels, so its PNG fits `MAX_SIDE`: a taller one
/// is an error.
const MAX_H: u32 = MAX_SIDE / SCALE;
/// The most pixels a diagram's PNG may have (64 million, 256 MB decoded): a bigger one is an
/// error rather than a screenshot Chrome and the view both have to hold.
const MAX_PIXELS: u64 = 64_000_000;
/// The wall clock after which a Chrome run that is still going is killed.
const BACKSTOP: Duration = Duration::from_secs(20);
/// How long the DOM is waited for once the first Chrome run has ended.
const READ_GRACE: Duration = Duration::from_secs(2);
/// The info line of a diagram whose render ended with no PNG and no message of its own.
const FAILED: &str = "the render failed";

/// Where one diagram is, for the rendered view, by its cache key: being rendered (or waiting
/// for its turn), in the cache with its size in CSS pixels, failed with mermaid's or Chrome's
/// message, or not to be rendered on this Mac, which has no Google Chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagram {
    Rendering,
    Ready { png: PathBuf, w: u32, h: u32 },
    Error(String),
    NoChrome,
}

/// The theme colours a diagram is drawn in, each as `#rrggbb`: mermaid's primary colour (the
/// node fill), the text, the lines, and the secondary and tertiary fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidTheme {
    pub primary: String,
    pub text: String,
    pub line: String,
    pub secondary: String,
    pub tertiary: String,
}

impl MermaidTheme {
    /// The colours from the chrome: the inactive tab for the nodes, the text for the text and
    /// the lines, the banner and the window's background for the other fills.
    pub fn from_chrome(chrome: &Chrome) -> MermaidTheme {
        MermaidTheme { primary: chrome.tab_inactive.hex(), text: chrome.text.hex(), line: chrome.text.hex(), secondary: chrome.banner.hex(), tertiary: chrome.window_bg.hex() }
    }

    /// The five values, in order.
    fn values(&self) -> [&str; 5] {
        [&self.primary, &self.text, &self.line, &self.secondary, &self.tertiary]
    }
}

/// Whether a fenced block's info string makes it a diagram: exactly `mermaid` once trimmed.
pub fn is_diagram_info(info: &str) -> bool {
    info.trim() == "mermaid"
}

/// The folder diagrams, their pages, the script and the render profiles are kept in, made by the
/// first render.
pub fn cache_dir() -> PathBuf {
    crate::cache::root().join("mermaid")
}

/// A diagram's cache key: the SHA-256 in hex of its source, the theme's five colour values and
/// the scale. The values, not the theme's name, since a user theme edited in place keeps its name.
pub fn cache_key(source: &str, theme: &MermaidTheme, scale: u32) -> String {
    let mut hash = Sha256::new();
    hash.update(source.as_bytes());
    for value in theme.values() {
        hash.update(b"\0");
        hash.update(value.as_bytes());
    }
    hash.update(b"\0");
    hash.update(scale.to_string().as_bytes());
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `text` with the characters HTML gives a meaning to written as entities.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A colour as it goes into the page's script: only `#` and hex digits are kept, so a value can
/// never end the string it sits in.
fn colour(value: &str) -> String {
    value.chars().filter(|c| *c == '#' || c.is_ascii_hexdigit()).collect()
}

/// The page Chrome renders `source` from, in `theme`'s colours: a content security policy that
/// allows inline code and local files only (no network, whatever the diagram holds), the bundled
/// script beside it, the source escaped in a `<pre class="mermaid">`, and a script that awaits
/// `mermaid.run()` and then puts the result in the title: `ok <w> <h>`, the diagram's size in
/// CSS pixels (its natural width, at most `MAX_W`), with the diagram and the page sized to it,
/// or `error <the first line of the message>`. The page and the diagram's background are
/// transparent.
pub fn page(source: &str, theme: &MermaidTheme) -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' file:; style-src 'unsafe-inline'; img-src data:">
<title></title>
<style>html, body {{ margin: 0; padding: 0; background: transparent; }} pre.mermaid {{ margin: 0; padding: 0; display: inline-block; }}</style>
<script src="./{SCRIPT_NAME}"></script>
</head>
<body>
<pre class="mermaid">{source}</pre>
<script>
mermaid.initialize({{ startOnLoad: false, theme: 'base', themeVariables: {{ background: 'transparent', primaryColor: '{primary}', primaryTextColor: '{text}', lineColor: '{line}', secondaryColor: '{secondary}', tertiaryColor: '{tertiary}', fontFamily: '-apple-system, system-ui' }} }});
(async () => {{
  try {{
    await mermaid.run({{ querySelector: 'pre.mermaid' }});
    const svg = document.querySelector('pre.mermaid svg');
    if (!svg) throw new Error('mermaid drew nothing');
    const box = svg.viewBox && svg.viewBox.baseVal && svg.viewBox.baseVal.width ? svg.viewBox.baseVal : svg.getBoundingClientRect();
    let w = box.width, h = box.height;
    if (w > {MAX_W}) {{ h = h * {MAX_W} / w; w = {MAX_W}; }}
    w = Math.max(1, Math.ceil(w)); h = Math.max(1, Math.ceil(h));
    svg.setAttribute('width', w); svg.setAttribute('height', h);
    svg.style.maxWidth = 'none'; svg.style.display = 'block';
    document.body.style.width = w + 'px'; document.body.style.height = h + 'px';
    document.title = 'ok ' + w + ' ' + h;
  }} catch (e) {{
    document.title = 'error ' + (String((e && e.message) || e).split('\n')[0] || '{FAILED}');
  }}
}})();
</script>
</body>
</html>
"#,
        source = escape_html(source),
        primary = colour(&theme.primary),
        text = colour(&theme.text),
        line = colour(&theme.line),
        secondary = colour(&theme.secondary),
        tertiary = colour(&theme.tertiary),
    )
}

/// The Google Chrome bundle: in `/Applications`, else in `~/Applications`, as the browser pane
/// finds it by name. This is the one place a path to it is needed.
pub fn chrome_app() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(|h| Path::new(&h).join("Applications/Google Chrome.app"));
    std::iter::once(PathBuf::from("/Applications/Google Chrome.app")).chain(home).find(|app| app.is_dir())
}

/// The Chrome program to render with, looked for once in the process; `None` when Google Chrome
/// is not installed.
pub fn chrome() -> Option<&'static Path> {
    static CHROME: OnceLock<Option<PathBuf>> = OnceLock::new();
    CHROME.get_or_init(|| chrome_app().map(|app| app.join("Contents/MacOS/Google Chrome")).filter(|exe| exe.is_file())).as_deref()
}

/// The throwaway Chrome profile of process `pid` in `dir`: made for each render and removed
/// after it; one a process left behind is swept once that process has ended.
pub fn profile_dir(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("profile-{pid}"))
}

/// The `file:` url of an absolute path, every byte but the unreserved ones and `/`
/// percent-encoded (the cache is under "Application Support").
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The flags of both Chrome runs: headless, with the user data in `profile`, and nothing that
/// reaches out: no background networking, component updates, sync, default apps, extensions or
/// crash reports, every host name resolving to nothing, and no keychain prompt. Virtual time lets the page's
/// script finish before Chrome reads the page, and `--timeout` ends a page that never settles.
pub fn common_args(profile: &Path) -> Vec<String> {
    [
        "--headless=new",
        "--disable-gpu",
        "--no-first-run",
        "--use-mock-keychain",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-sync",
        "--disable-default-apps",
        "--disable-extensions",
        "--no-default-browser-check",
        "--disable-breakpad",
        "--host-resolver-rules=MAP * ~NOTFOUND",
    ]
    .into_iter()
    .map(String::from)
    .chain([format!("--user-data-dir={}", profile.display()), "--virtual-time-budget=5000".into(), "--timeout=15000".into()])
    .collect()
}

/// The first run's command line, the program first: `page` loaded and its DOM printed on
/// stdout, where `size_from_dom` reads the title.
pub fn dump_args(chrome: &Path, profile: &Path, page: &Path) -> Vec<String> {
    let mut args = vec![chrome.to_string_lossy().into_owned()];
    args.extend(common_args(profile));
    args.extend(["--dump-dom".into(), file_url(page)]);
    args
}

/// The second run's command line, the program first: `page` in a window of `w` by `h` CSS
/// pixels at `SCALE` times the pixels, on a transparent background with no scroll bars,
/// screenshot into `out` (a PNG of `SCALE * w` by `SCALE * h`).
pub fn shot_args(chrome: &Path, profile: &Path, page: &Path, out: &Path, w: u32, h: u32) -> Vec<String> {
    let mut args = vec![chrome.to_string_lossy().into_owned()];
    args.extend(common_args(profile));
    args.extend([
        "--hide-scrollbars".into(),
        format!("--force-device-scale-factor={SCALE}"),
        "--default-background-color=00000000".into(),
        format!("--window-size={w},{h}"),
        format!("--screenshot={}", out.display()),
        file_url(page),
    ]);
    args
}

/// `text` with the entities Chrome writes in a page's text read back: the named ones HTML text
/// needs and numeric ones. An entity not known is left as it is.
fn unescape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let entity = rest.find(';').map(|end| (&rest[1..end], end));
        let decoded = entity.and_then(|(name, end)| {
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok().and_then(char::from_u32),
                    None => name.strip_prefix('#').and_then(|d| d.parse().ok()).and_then(char::from_u32),
                },
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// What the page's script put in its title, from the DOM Chrome printed: the diagram's size in
/// CSS pixels for `ok <w> <h>`, the message for `error <message>`, and an error when the title
/// is neither (the script did not finish within the virtual time).
pub fn size_from_dom(dom: &str) -> Result<(u32, u32), String> {
    let title = dom.find("<title>").and_then(|at| {
        let rest = &dom[at + "<title>".len()..];
        rest.find("</title>").map(|end| unescape_html(&rest[..end]))
    });
    let title = title.ok_or_else(|| "the page has no title".to_string())?;
    if let Some(message) = title.strip_prefix("error ") {
        return Err(message.trim().to_string());
    }
    let size = title.strip_prefix("ok ").and_then(|wh| {
        let mut parts = wh.split_whitespace().map(str::parse::<u32>);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(Ok(w)), Some(Ok(h)), None) if w > 0 && h > 0 => Some((w, h)),
            _ => None,
        }
    });
    size.ok_or_else(|| "mermaid did not finish".to_string())
}

/// The PNG and the size file of the diagram with `key` in `dir`.
fn cache_files(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (dir.join(format!("{key}.png")), dir.join(format!("{key}.json")))
}

/// The diagram with `key` when a render (in this process or another) left it in `dir`: its PNG,
/// with the size from the `{ "w", "h" }` file beside it.
pub fn cached(dir: &Path, key: &str) -> Option<Diagram> {
    let (png, size) = cache_files(dir, key);
    if !png.is_file() {
        return None;
    }
    let size: serde_json::Value = serde_json::from_slice(&std::fs::read(size).ok()?).ok()?;
    let side = |name: &str| size.get(name)?.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v > 0);
    Some(Diagram::Ready { png, w: side("w")?, h: side("h")? })
}

/// One render to run: the diagram's cache key and the page it is drawn from. Jobs are the same
/// when their keys are (the page follows from the key).
#[derive(Debug, Clone)]
pub struct Job {
    pub key: String,
    pub page: String,
}

impl PartialEq for Job {
    fn eq(&self, other: &Job) -> bool {
        self.key == other.key
    }
}

impl Eq for Job {}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Job) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Job {
    fn cmp(&self, other: &Job) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl Borrow<str> for Job {
    fn borrow(&self) -> &str {
        &self.key
    }
}

/// The process's diagram renders, one at a time.
static RENDERS: Mutex<FetchQueue<Job>> = Mutex::new(FetchQueue::new(1));

/// The messages of the renders that failed in this process, by key, for every view waiting on
/// one. Wanting the key again forgets its message.
static ERRORS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// The process-wide render queue, locked. A panic while it was held leaves it as it was.
pub fn queue() -> MutexGuard<'static, FetchQueue<Job>> {
    RENDERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The failed renders' messages, locked.
fn errors() -> MutexGuard<'static, BTreeMap<String, String>> {
    ERRORS.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the diagram with `key` the rendered view shows becomes, `None` when it stays as it is:
/// one not seen yet (`state` is `None`) is ready when an earlier render left it in the cache,
/// not to be rendered when there is no Chrome, else queued with its page and rendering; one
/// rendering whose job has ended (the queue no longer has it) is ready or failed by what the
/// cache holds. `page`, `dir` and `has_chrome` are asked for only when they are needed.
pub fn advance(state: Option<&Diagram>, key: &str, page: impl FnOnce() -> String, dir: impl FnOnce() -> PathBuf, has_chrome: impl FnOnce() -> bool) -> Option<Diagram> {
    match state {
        None => {
            // One guard from the pending check to the want: a render that lands in between is
            // seen in the cache, never started a second time.
            let mut queue = queue();
            if queue.is_pending(key) {
                return Some(Diagram::Rendering);
            }
            Some(match cached(&dir(), key) {
                Some(ready) => ready,
                None if !has_chrome() => Diagram::NoChrome,
                None => {
                    errors().remove(key);
                    queue.want(Job { key: key.to_string(), page: page() });
                    Diagram::Rendering
                }
            })
        }
        Some(Diagram::Rendering) if !queue().is_pending(key) => Some(cached(&dir(), key).unwrap_or_else(|| Diagram::Error(errors().get(key).cloned().unwrap_or_else(|| FAILED.to_string())))),
        Some(_) => None,
    }
}

/// Runs `job` with the process's Chrome into `dir`, blocking the calling thread; a failure's
/// message is kept for the views waiting on it (`advance`). Frees the job's turn either way.
pub fn run_job(dir: &Path, job: &Job) {
    let rendered = chrome().ok_or_else(|| "Google Chrome is not installed".to_string()).and_then(|chrome| render(dir, chrome, job));
    if let Err(e) = rendered {
        eprintln!("arkos: cannot render a mermaid diagram (mermaid {VERSION}): {e}");
        errors().insert(job.key.clone(), e);
    }
    queue().done(job.key.as_str());
}

/// Renders `job` with `chrome` into `dir` (made when missing), blocking the calling thread: the
/// bundled script written out once, the page written to `<key>.<pid>.html` (two windows may
/// render the same diagram), the size read in a first run and the screenshot taken in a second
/// into `<key>.<pid>.png`, then the size file and the PNG renamed into place, the size first so
/// a PNG is never there without it. The page, the profile and anything half made are removed
/// either way. Returns the diagram's size in CSS pixels.
pub fn render(dir: &Path, chrome: &Path, job: &Job) -> Result<(u32, u32), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    write_script(dir)?;
    let pid = std::process::id();
    let page = dir.join(format!("{}.{pid}.html", job.key));
    let shot = dir.join(format!("{}.{pid}.png", job.key));
    let size_part = dir.join(format!("{}.{pid}.json", job.key));
    let (png, size) = cache_files(dir, &job.key);
    let profile = profile_dir(dir, pid);
    // A folder left by an earlier process with this pid that ended in the middle of a render.
    let _ = std::fs::remove_dir_all(&profile);
    let rendered = std::fs::create_dir_all(&profile)
        .and_then(|()| std::fs::write(&page, &job.page))
        .map_err(|e| e.to_string())
        .and_then(|()| run_chrome(&dump_args(chrome, &profile, &page), true))
        .and_then(|dom| size_from_dom(&dom))
        .and_then(|(w, h)| fits(w, h).map(|()| (w, h)))
        .and_then(|(w, h)| run_chrome(&shot_args(chrome, &profile, &page, &shot, w, h), false).map(|_| (w, h)))
        .and_then(|(w, h)| {
            if !std::fs::metadata(&shot).is_ok_and(|m| m.len() > 0) {
                return Err("Chrome made no screenshot".to_string());
            }
            std::fs::write(&size_part, serde_json::json!({ "w": w, "h": h }).to_string())
                .and_then(|()| std::fs::rename(&size_part, &size))
                .and_then(|()| std::fs::rename(&shot, &png))
                .map_err(|e| e.to_string())
                .map(|()| (w, h))
        });
    for leftover in [&page, &shot, &size_part] {
        let _ = std::fs::remove_file(leftover);
    }
    let _ = std::fs::remove_dir_all(&profile);
    rendered
}

/// Whether a diagram of `w` by `h` CSS pixels can be screenshot and drawn: its PNG's sides at
/// most `MAX_SIDE` and its pixels at most `MAX_PIXELS`; else the error the block shows.
fn fits(w: u32, h: u32) -> Result<(), String> {
    if w.max(h) > MAX_H {
        return Err(format!("the diagram is {w} by {h} pixels, more than {MAX_H} on a side"));
    }
    if u64::from(w) * u64::from(h) * u64::from(SCALE * SCALE) > MAX_PIXELS {
        return Err(format!("the diagram is {w} by {h} pixels, too large to draw"));
    }
    Ok(())
}

/// Writes the bundled script into `dir` as `SCRIPT_NAME` unless it is there: through a file of
/// the process's own and a rename, so a page never loads half a script. Writing it removes the
/// scripts of other versions (`old_scripts`), which no page loads any more.
fn write_script(dir: &Path) -> Result<(), String> {
    let script = dir.join(SCRIPT_NAME);
    if script.is_file() {
        return Ok(());
    }
    let part = dir.join(format!("{SCRIPT_NAME}.{}.part", std::process::id()));
    let written = std::fs::write(&part, SCRIPT).and_then(|()| std::fs::rename(&part, &script)).map_err(|e| format!("cannot write {}: {e}", script.display()));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    for old in old_scripts(dir) {
        let _ = std::fs::remove_file(old);
    }
    written
}

/// The plain files in `dir` named as a mermaid script (`mermaid-<version>.min.js`) of a version
/// other than this one's.
fn old_scripts(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
        .filter(|e| e.file_name().to_str().is_some_and(|n| n != SCRIPT_NAME && n.starts_with("mermaid-") && n.ends_with(".min.js")))
        .map(|e| e.path())
        .collect()
}

/// Runs one Chrome command line (`dump_args` or `shot_args`, the program first) with a minimal
/// environment in a process group of its own, killed with the group once `BACKSTOP` passes.
/// Returns what it printed on stdout when `capture` is set (read on a thread of its own, so a
/// large page cannot fill the pipe while the wait goes on), else nothing.
fn run_chrome(command: &[String], capture: bool) -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let (program, args) = command.split_first().ok_or_else(|| "no command".to_string())?;
    let mut cmd = Command::new(program);
    cmd.args(args).env_clear().env("PATH", "/usr/bin:/bin");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    let stdout = if capture { Stdio::piped() } else { Stdio::null() };
    let mut child = cmd.stdin(Stdio::null()).stdout(stdout).stderr(Stdio::null()).process_group(0).spawn().map_err(|e| format!("cannot run Google Chrome: {e}"))?;
    let reader = child.stdout.take().map(|mut pipe| {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = String::new();
            let _ = pipe.read_to_string(&mut out);
            let _ = send.send(out);
        });
        receive
    });
    let waited = remote_image::wait_bounded(&mut child, BACKSTOP, "Google Chrome");
    // A helper Chrome started could hold the pipe open after Chrome itself ended: the reader is
    // given a moment, not waited for.
    let out = reader.and_then(|r| r.recv_timeout(READ_GRACE).ok()).unwrap_or_default();
    let status = waited?;
    if status.success() {
        Ok(out)
    } else {
        Err(format!("Google Chrome: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_page_carries_the_theme_the_csp_and_the_escaped_source_and_the_arguments_are_fixed() {
        let t = MermaidTheme { primary: "#2a0062".into(), text: "#f3edff".into(), line: "#f3edff".into(), secondary: "#3b1a86".into(), tertiary: "#330076".into() };
        let html = page("graph TD; A-->B & <C>", &t);
        assert!(html.contains("primaryColor: '#2a0062'") && html.contains("fontFamily: '-apple-system, system-ui'"));
        assert!(html.contains("A--&gt;B &amp; &lt;C&gt;"), "escaped: {html}");
        assert!(html.contains(&format!("<script src=\"./{SCRIPT_NAME}\">")));
        assert!(html.contains("Content-Security-Policy") && html.contains("default-src 'none'"));
        assert!(html.contains("await mermaid.run"));
        assert_eq!(cache_key("a", &t, 2), cache_key("a", &t, 2));
        let t2 = MermaidTheme { primary: "#000000".into(), ..t.clone() };
        assert_ne!(cache_key("a", &t, 2), cache_key("a", &t2, 2), "the values, not a name");
        let c = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        let common = common_args(Path::new("/p"));
        for flag in ["--headless=new", "--disable-gpu", "--no-first-run", "--use-mock-keychain", "--disable-background-networking", "--disable-component-update", "--disable-sync", "--disable-default-apps", "--disable-extensions", "--no-default-browser-check", "--disable-breakpad", "--host-resolver-rules=MAP * ~NOTFOUND", "--user-data-dir=/p", "--virtual-time-budget=5000", "--timeout=15000"] {
            assert!(common.contains(&flag.to_string()), "{flag}");
        }
        let d = dump_args(c, Path::new("/p"), Path::new("/k.html"));
        assert!(d.contains(&"--dump-dom".to_string()) && d.last().unwrap() == "file:///k.html");
        let s = shot_args(c, Path::new("/p"), Path::new("/k.html"), Path::new("/k.png"), 640, 480);
        for flag in ["--window-size=640,480", "--screenshot=/k.png", "--force-device-scale-factor=2", "--default-background-color=00000000", "--hide-scrollbars"] {
            assert!(s.contains(&flag.to_string()), "{flag}");
        }
        assert_eq!(size_from_dom("<html><head><title>ok 640 480</title></head></html>"), Ok((640, 480)));
        assert_eq!(size_from_dom("<title>error Parse error on line 2: &lt;x&gt;</title>"), Err("Parse error on line 2: <x>".into()));
        assert!(size_from_dom("<title></title>").is_err());
        assert!(is_diagram_info("mermaid") && is_diagram_info(" mermaid ") && !is_diagram_info("mermaidjs"));
        assert_eq!(SCRIPT_NAME, format!("mermaid-{VERSION}.min.js"));
        assert!(profile_dir(Path::new("/c"), 42).ends_with("profile-42"));
    }

    #[test]
    fn a_diagram_is_ready_from_the_cache_or_rendered_once_until_its_job_ends() {
        let dir = tempfile::tempdir().unwrap();
        let at = || dir.path().to_path_buf();
        let page = || "<p>".to_string();
        let (old, new, gone) = ("test-cached-diagram", "test-fresh-diagram", "test-no-chrome-diagram");
        std::fs::write(dir.path().join(format!("{old}.png")), "png").unwrap();
        std::fs::write(dir.path().join(format!("{old}.json")), r#"{"w":320,"h":200}"#).unwrap();
        let ready = Diagram::Ready { png: dir.path().join(format!("{old}.png")), w: 320, h: 200 };
        assert_eq!(advance(None, old, page, at, || panic!("no Chrome check for a cached diagram")), Some(ready.clone()), "a diagram an earlier render left is used at once, with its size");
        assert!(!queue().is_pending(old));
        assert_eq!(advance(None, gone, page, at, || false), Some(Diagram::NoChrome));
        assert!(!queue().is_pending(gone), "nothing is queued without Chrome");
        assert_eq!(advance(None, new, page, at, || true), Some(Diagram::Rendering));
        assert!(queue().is_pending(new), "a diagram not in the cache is queued");
        assert_eq!(advance(None, new, || panic!("no second page"), at, || true), Some(Diagram::Rendering), "a second view waits on the same job");
        assert_eq!(advance(Some(&Diagram::Rendering), new, page, at, || true), None, "it stays rendering while queued");
        errors().insert(new.to_string(), "Parse error on line 2:".into());
        queue().done(new);
        assert_eq!(advance(Some(&Diagram::Rendering), new, page, at, || true), Some(Diagram::Error("Parse error on line 2:".into())));
        assert_eq!(advance(Some(&Diagram::Error("x".into())), new, page, at, || true), None, "a failed diagram is not rendered again in this view");
        std::fs::write(dir.path().join(format!("{new}.png")), "png").unwrap();
        assert_eq!(advance(Some(&Diagram::Rendering), new, page, at, || true), Some(Diagram::Error("Parse error on line 2:".into())), "a PNG without its size file is not ready");
        std::fs::write(dir.path().join(format!("{new}.json")), r#"{"w":10,"h":20}"#).unwrap();
        assert_eq!(advance(Some(&Diagram::Rendering), new, page, at, || true), Some(Diagram::Ready { png: dir.path().join(format!("{new}.png")), w: 10, h: 20 }));
    }

    #[test]
    fn the_theme_comes_from_the_chrome_and_the_page_url_is_encoded() {
        let theme = crate::theme::builtin("navy").unwrap();
        let t = MermaidTheme::from_chrome(&theme.chrome);
        assert_eq!(t, MermaidTheme { primary: theme.chrome.tab_inactive.hex(), text: theme.chrome.text.hex(), line: theme.chrome.text.hex(), secondary: theme.chrome.banner.hex(), tertiary: theme.chrome.window_bg.hex() });
        let d = dump_args(Path::new("/c"), Path::new("/p"), Path::new("/Users/a/Library/Application Support/arkos/cache/mermaid/k.1.html"));
        assert_eq!(d.first().unwrap(), "/c", "the program first");
        assert_eq!(d.last().unwrap(), "file:///Users/a/Library/Application%20Support/arkos/cache/mermaid/k.1.html");
        assert!(!page("a", &MermaidTheme { primary: "#fff'); alert(1); ('".into(), ..t }).contains("alert(1)"), "a colour cannot end its string");
        assert_eq!(size_from_dom("<title>ok 0 10</title>"), Err("mermaid did not finish".into()));
        assert_eq!(size_from_dom("<title>error a &amp; b &#39;c&#x27;</title>"), Err("a & b 'c'".into()));
    }

    #[test]
    fn a_diagram_too_large_to_draw_is_refused_and_old_scripts_go() {
        assert!(MAX_H * SCALE <= 16384, "a PNG fits gpui's atlas");
        assert_eq!(fits(2000, MAX_H), Err(format!("the diagram is 2000 by {MAX_H} pixels, too large to draw")), "the area is capped too");
        assert!(fits(2000, 4000).is_ok() && fits(100, MAX_H).is_ok());
        assert!(fits(100, MAX_H + 1).is_err() && fits(MAX_H + 1, 10).is_err());
        let dir = tempfile::tempdir().unwrap();
        for name in ["mermaid-10.0.0.min.js", SCRIPT_NAME, "mermaid-x.js", "k.png"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("mermaid-9.min.js")).unwrap();
        assert_eq!(old_scripts(dir.path()), [dir.path().join("mermaid-10.0.0.min.js")], "only another version's script file");
    }
}
