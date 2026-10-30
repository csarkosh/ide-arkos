//! Mermaid diagrams in the rendered view (spec 6; diagrams and math spec, section 1): a fenced
//! code block whose info string is `mermaid` is drawn by the window's own hidden web view
//! (`render_view`) from a page Arkos writes into `<cache>/mermaid/` beside the bundled mermaid
//! script, to a PNG cached by the SHA-256 of the source, the theme's colours and the scale
//! (`<key>.png`, with its size in `<key>.json`). The page's script puts `ok <w> <h>` or
//! `error <message>` in its title, and the view takes a snapshot of that size. Nothing leaves the
//! machine: the page is a local file whose content security policy allows only local files and its
//! own inline script (by its hash). One render runs at a time in the process (a window is a
//! process), through the `queue()` gate, and only for a block drawn rendered, never for the one
//! being typed in; the folder is swept at launch (`cache::sweep_all`).

use crate::remote_image::FetchQueue;
use crate::theme::Chrome;
use sha2::{Digest, Sha256};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

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
/// The scale of the snapshot: a diagram's PNG has twice the pixels of its size in CSS pixels on
/// any screen, and is drawn at that size.
pub const SCALE: u32 = 2;
/// The widest a diagram is drawn, in CSS pixels; a wider one is scaled down to it.
pub const MAX_W: u32 = 2000;
/// The longest side, in pixels, of an image gpui can draw (its Metal atlas's limit): a PNG with
/// a longer side would be ready and draw nothing.
const MAX_SIDE: u32 = 16_384;
/// The tallest diagram that is rendered, in CSS pixels, so its PNG fits `MAX_SIDE`: a taller one
/// is an error.
const MAX_H: u32 = MAX_SIDE / SCALE;
/// The most pixels a diagram's PNG may have (64 million, 256 MB decoded): a bigger one is an
/// error rather than a snapshot the web view and the rendered view both have to hold.
const MAX_PIXELS: u64 = 64_000_000;
/// The info line of a diagram whose render ended with no PNG and no message of its own.
const FAILED: &str = "the render failed";

/// Where one diagram is, for the rendered view, by its cache key: being rendered (or waiting
/// for its turn), in the cache with its size in CSS pixels, failed with mermaid's or the web
/// view's message, or not to be rendered in this window, which has no web view to draw with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagram {
    Rendering,
    Ready { png: PathBuf, w: u32, h: u32 },
    Error(String),
    NoRenderer,
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

/// The page the web view renders `source` from, in `theme`'s colours: a content security policy that
/// allows only local files and the one inline script the page holds, by its hash (no network,
/// and no other script, whatever the diagram holds), the bundled script beside it, the source
/// escaped in a `<pre class="mermaid">`, and a script that awaits `mermaid.run()` and then puts
/// the result in the title: `ok <w> <h>`, the diagram's size in CSS pixels (its natural width, at
/// most `MAX_W`), with the diagram and the page sized to it, or `error <the first line of the
/// message>`. The page and the diagram's background are transparent.
pub fn page(source: &str, theme: &MermaidTheme) -> String {
    let script = format!(
        r#"
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
"#,
        primary = colour(&theme.primary),
        text = colour(&theme.text),
        line = colour(&theme.line),
        secondary = colour(&theme.secondary),
        tertiary = colour(&theme.tertiary),
    );
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src {hash} file:; style-src 'unsafe-inline'; img-src data:">
<title></title>
<style>html, body {{ margin: 0; padding: 0; background: transparent; }} pre.mermaid {{ margin: 0; padding: 0; display: inline-block; }}</style>
<script src="./{SCRIPT_NAME}"></script>
</head>
<body>
<pre class="mermaid">{source}</pre>
<script>{script}</script>
</body>
</html>
"#,
        hash = script_hash(&script),
        source = escape_html(source),
    )
}

/// A content security policy source that allows the inline script `script` and nothing else:
/// `'sha256-<the base64 of its SHA-256>'`, over the exact text between its tags.
pub(crate) fn script_hash(script: &str) -> String {
    format!("'sha256-{}'", base64(&Sha256::digest(script.as_bytes())))
}

/// Standard base64 with padding, the encoding a CSP hash source takes.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// What the page's script put in its title: the diagram's size in CSS pixels for `ok <w> <h>`,
/// the message for `error <message>`, and `None` while the title is neither (the script has not
/// finished).
pub fn size_from_title(title: &str) -> Result<Option<(u32, u32)>, String> {
    if let Some(message) = title.strip_prefix("error ") {
        let message = message.trim();
        return Err(if message.is_empty() { FAILED.to_string() } else { message.to_string() });
    }
    let Some(wh) = title.strip_prefix("ok ") else { return Ok(None) };
    let mut parts = wh.split_whitespace().map(str::parse::<u32>);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(w)), Some(Ok(h)), None) if w > 0 && h > 0 => Ok(Some((w, h))),
        _ => Err(format!("the page gave no size: {title}")),
    }
}

/// The PNG and the size file of the diagram with `key` in `dir`.
fn cache_files(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (dir.join(format!("{key}.png")), dir.join(format!("{key}.json")))
}

/// The diagram with `key` when a render (in this process or another) left it in `dir` and it is
/// not near its sweep (`cache::fresh`): its PNG, with the size from the `{ "w", "h" }` file beside
/// it.
pub fn cached(dir: &Path, key: &str) -> Option<Diagram> {
    let (png, size) = cache_files(dir, key);
    if !crate::cache::fresh(&png, crate::cache::MERMAID_AGE) || !crate::cache::fresh(&size, crate::cache::MERMAID_AGE) {
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
/// not to be rendered when the window cannot draw, else queued with its page and rendering; one
/// rendering whose job has ended (the queue no longer has it) is ready or failed by what the
/// cache holds. `page`, `dir` and `can_render` are asked for only when they are needed.
pub fn advance(state: Option<&Diagram>, key: &str, page: impl FnOnce() -> String, dir: impl FnOnce() -> PathBuf, can_render: impl FnOnce() -> bool) -> Option<Diagram> {
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
                None if !can_render() => Diagram::NoRenderer,
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

/// Writes the bundled scripts and `job`'s page into `dir` (made when missing), the page as
/// `<key>.<pid>.html` (two windows may render the same diagram); returns the page's path.
pub fn prepare(dir: &Path, job: &Job) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    write_script(dir)?;
    crate::md_math::write_script(dir)?;
    let page = dir.join(format!("{}.{}.html", job.key, std::process::id()));
    std::fs::write(&page, &job.page).map_err(|e| format!("cannot write {}: {e}", page.display()))?;
    Ok(page)
}

/// Ends the render of `key` from `page`: its size and PNG written into the cache (the size file
/// first, each through a file of the process's own and a rename, so a PNG is never there without
/// its size), or its message kept for the views waiting on it (`advance`). The page goes, and the
/// turn is freed, either way.
pub fn finish(dir: &Path, key: &str, page: &Path, result: Result<(u32, u32, Vec<u8>), String>) {
    let pid = std::process::id();
    let (png, size) = cache_files(dir, key);
    let (png_part, size_part) = (dir.join(format!("{key}.{pid}.png")), dir.join(format!("{key}.{pid}.json")));
    let written = result.and_then(|(w, h, bytes)| {
        std::fs::write(&size_part, serde_json::json!({ "w": w, "h": h }).to_string())
            .and_then(|()| std::fs::write(&png_part, bytes))
            .and_then(|()| std::fs::rename(&size_part, &size))
            .and_then(|()| std::fs::rename(&png_part, &png))
            .map_err(|e| e.to_string())
    });
    if let Err(e) = written {
        eprintln!("arkos: cannot render a diagram (mermaid {VERSION}): {e}");
        errors().insert(key.to_string(), e);
    }
    for leftover in [page, &png_part, &size_part] {
        let _ = std::fs::remove_file(leftover);
    }
    queue().done(key);
}

/// Whether a diagram of `w` by `h` CSS pixels can be snapshot and drawn: its PNG's sides at
/// most `MAX_SIDE` and its pixels at most `MAX_PIXELS`; else the error the block shows.
pub fn fits(w: u32, h: u32) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_carries_the_theme_the_csp_and_the_escaped_source_and_the_title_says_how_it_went() {
        let t = MermaidTheme { primary: "#2a0062".into(), text: "#f3edff".into(), line: "#f3edff".into(), secondary: "#3b1a86".into(), tertiary: "#330076".into() };
        let html = page("graph TD; A-->B & <C>", &t);
        assert!(html.contains("primaryColor: '#2a0062'") && html.contains("fontFamily: '-apple-system, system-ui'"));
        assert!(html.contains("A--&gt;B &amp; &lt;C&gt;"), "escaped: {html}");
        assert!(html.contains(&format!("<script src=\"./{SCRIPT_NAME}\">")));
        assert!(html.contains("Content-Security-Policy") && html.contains("default-src 'none'"));
        assert!(html.contains("await mermaid.run"));
        // The policy allows the page's one inline script by its hash, and no inline script else.
        let script = html.split("<script>").nth(1).and_then(|rest| rest.split("</script>").next()).expect("the inline script");
        assert!(script.contains("mermaid.initialize") && script.contains("'#2a0062'"));
        let hash = format!("'sha256-{}'", base64(&Sha256::digest(script.as_bytes())));
        assert!(html.contains(&format!("script-src {hash} file:;")), "{html}");
        assert!(!html.contains("script-src 'unsafe-inline'"));
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&Sha256::digest(b"abc")), "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=", "the SHA-256 of abc, as a CSP hash writes it");
        assert_eq!(cache_key("a", &t, 2), cache_key("a", &t, 2));
        let t2 = MermaidTheme { primary: "#000000".into(), ..t.clone() };
        assert_ne!(cache_key("a", &t, 2), cache_key("a", &t2, 2), "the values, not a name");
        assert_eq!(size_from_title("ok 640 480"), Ok(Some((640, 480))));
        assert_eq!(size_from_title(""), Ok(None), "not finished yet");
        assert_eq!(size_from_title("error Parse error on line 2: <x>"), Err("Parse error on line 2: <x>".into()));
        assert_eq!(size_from_title("error "), Err(FAILED.into()));
        assert!(size_from_title("ok 0 10").is_err() && size_from_title("ok 1 2 3").is_err());
        assert!(is_diagram_info("mermaid") && is_diagram_info(" mermaid ") && !is_diagram_info("mermaidjs"));
        assert_eq!(SCRIPT_NAME, format!("mermaid-{VERSION}.min.js"));
    }

    #[test]
    fn a_finished_render_lands_in_the_cache_or_leaves_its_message_and_frees_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let job = Job { key: "test-finish-diagram".into(), page: "<p>".into() };
        queue().want(job.clone());
        let page = prepare(dir.path(), &job).unwrap();
        assert!(page.is_file() && dir.path().join(SCRIPT_NAME).is_file());
        finish(dir.path(), &job.key, &page, Ok((12, 8, b"png".to_vec())));
        assert!(!page.exists() && !queue().is_pending(job.key.as_str()));
        assert_eq!(cached(dir.path(), &job.key), Some(Diagram::Ready { png: dir.path().join("test-finish-diagram.png"), w: 12, h: 8 }));
        let bad = Job { key: "test-finish-error".into(), page: "<p>".into() };
        queue().want(bad.clone());
        let page = prepare(dir.path(), &bad).unwrap();
        finish(dir.path(), &bad.key, &page, Err("Parse error".into()));
        assert_eq!(advance(Some(&Diagram::Rendering), &bad.key, || unreachable!(), || dir.path().to_path_buf(), || true), Some(Diagram::Error("Parse error".into())));
    }

    #[test]
    fn a_diagram_is_ready_from_the_cache_or_rendered_once_until_its_job_ends() {
        let dir = tempfile::tempdir().unwrap();
        let at = || dir.path().to_path_buf();
        let page = || "<p>".to_string();
        let (old, new, gone) = ("test-cached-diagram", "test-fresh-diagram", "test-no-renderer-diagram");
        std::fs::write(dir.path().join(format!("{old}.png")), "png").unwrap();
        std::fs::write(dir.path().join(format!("{old}.json")), r#"{"w":320,"h":200}"#).unwrap();
        let ready = Diagram::Ready { png: dir.path().join(format!("{old}.png")), w: 320, h: 200 };
        assert_eq!(advance(None, old, page, at, || panic!("no renderer check for a cached diagram")), Some(ready.clone()), "a diagram an earlier render left is used at once, with its size");
        assert!(!queue().is_pending(old));
        assert_eq!(advance(None, gone, page, at, || false), Some(Diagram::NoRenderer));
        assert!(!queue().is_pending(gone), "nothing is queued without a renderer");
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
    fn a_diagram_near_its_sweep_age_is_rendered_again() {
        let dir = tempfile::tempdir().unwrap();
        let (png, size) = cache_files(dir.path(), "test-old-diagram");
        std::fs::write(&png, "png").unwrap();
        std::fs::write(&size, r#"{"w":4,"h":3}"#).unwrap();
        assert!(cached(dir.path(), "test-old-diagram").is_some());
        let old = std::time::SystemTime::now() - (crate::cache::MERMAID_AGE - std::time::Duration::from_secs(3600));
        std::fs::File::options().write(true).open(&png).unwrap().set_modified(old).unwrap();
        assert_eq!(cached(dir.path(), "test-old-diagram"), None, "within a day of the sweep: missing, so it is rendered again");
    }

    #[test]
    fn the_theme_comes_from_the_chrome_and_a_colour_cannot_end_its_string() {
        let theme = crate::theme::builtin("navy").unwrap();
        let t = MermaidTheme::from_chrome(&theme.chrome);
        assert_eq!(t, MermaidTheme { primary: theme.chrome.tab_inactive.hex(), text: theme.chrome.text.hex(), line: theme.chrome.text.hex(), secondary: theme.chrome.banner.hex(), tertiary: theme.chrome.window_bg.hex() });
        assert!(!page("a", &MermaidTheme { primary: "#fff'); alert(1); ('".into(), ..t }).contains("alert(1)"), "a colour cannot end its string");
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
