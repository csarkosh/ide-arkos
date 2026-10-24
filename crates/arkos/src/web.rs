//! The web pane's rules (web pane spec, sections 3 to 6), pure and tested: what a typed address
//! means, what a local page may read, a tab's title, the zoom ladder, the `Web` trait the real
//! `WKWebView` host implements (`wk_web_view`) with a fake for the tests, and `WebModel`, the
//! state of one web tab: blank, loading, ready or failed, the committed URL the workspace keeps,
//! and a local page's follow of its file. Nothing here touches WebKit or gpui.
//!
//! The next iteration (agent-driven browsing, spec section 12) adds two calls to the trait,
//! `evaluate_js` and `snapshot`, and nothing else here has to change for it.

use crate::screen::Frame;
use crate::video::{follow, Follow, Fp};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The bar above the page, the video strip's height.
pub const BAR_H: f32 = 28.0;

/// Safari's zoom steps.
pub const ZOOM_LADDER: &[f64] = &[0.5, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// A second crash of the page's process within this long shows the failure instead of a third
/// reload.
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);

/// How long a notice ("downloads are not supported here") stays on the bar.
pub const NOTICE_FOR: Duration = Duration::from_secs(6);

/// The next zoom level up (`dir` positive) or down from `current`; the ladder's ends hold.
pub fn zoom_step(current: f64, dir: i32) -> f64 {
    let i = ZOOM_LADDER.iter().position(|z| (*z - current).abs() < 1e-6).unwrap_or_else(|| {
        // Off the ladder (a value set some other way): the nearest rung.
        ZOOM_LADDER.iter().enumerate().min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs())).map_or(3, |(i, _)| i)
    });
    let j = if dir > 0 { (i + 1).min(ZOOM_LADDER.len() - 1) } else { i.saturating_sub(1) };
    ZOOM_LADDER[j]
}

/// Whether `host` is this machine, which gets `http://` rather than `https://`.
fn is_local_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host == "127.0.0.1" || host == "[::1]" || host == "0.0.0.0"
}

/// What a typed or given address means (spec 5.1): an `http(s)` or `file` URL as it is; a path
/// (absolute, `~/`, or relative to `cwd`) that names a file as its `file://` URL; a bare host
/// (this machine, a host with a port, a dotted word) with a scheme added; anything else is
/// refused with the reason. Never a search.
pub fn parse_address(text: &str, cwd: &Path) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("nothing to open".into());
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Ok(text.to_string());
    }
    if text.starts_with("file://") {
        return Ok(text.to_string());
    }
    if let Some((scheme, rest)) = text.split_once(':') {
        // `host:port` is not a scheme: what follows the colon is a number.
        let port_like = rest.split('/').next().is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        if scheme.len() > 1 && scheme.chars().all(|c| c.is_ascii_alphabetic()) && !port_like && !text.contains(' ') && !Path::new(text).exists() {
            return Err(format!("{scheme}: addresses open outside Arkos"));
        }
    }
    let path = if let Some(rest) = text.strip_prefix("~/") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest))
    } else if text.starts_with('/') {
        Some(PathBuf::from(text))
    } else if text.starts_with("./") || text.starts_with("../") || text.ends_with(".html") || text.ends_with(".htm") {
        Some(cwd.join(text))
    } else {
        None
    };
    if let Some(path) = path {
        return match path.canonicalize() {
            Ok(abs) if abs.is_file() => Ok(file_url(&abs)),
            Ok(abs) => Err(format!("{} is a folder", abs.display())),
            Err(_) => Err(format!("no such file: {}", path.display())),
        };
    }
    if text.contains(' ') {
        return Err(format!("not an address: {text:?}"));
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) if !n.ends_with(']') || n.starts_with('[') => (n, Some(p)),
        _ => (host, None),
    };
    if port.is_some_and(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("not an address: {text:?}"));
    }
    if is_local_host(name) || (port.is_some() && !name.is_empty()) {
        return Ok(format!("http://{text}"));
    }
    if name.contains('.') && !name.starts_with('.') && !name.ends_with('.') {
        return Ok(format!("https://{text}"));
    }
    Err(format!("not an address: {text:?}"))
}

/// The folder a local page may read (spec 5.2, ruling 10): the project when the file is inside
/// it, else the file's own folder.
pub fn read_access_root(file: &Path, project: &Path) -> PathBuf {
    if file.starts_with(project) {
        project.to_path_buf()
    } else {
        file.parent().map_or_else(|| file.to_path_buf(), Path::to_path_buf)
    }
}

/// A `file://` URL for an absolute path, with the characters a URL cannot carry escaped.
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The path of a `file://` URL, unescaped; `None` for any other URL.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&raw[i + 1..i + 3]).unwrap_or(""), 16) {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

/// The host of an absolute URL, without user info or port; `None` when there is none.
fn url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        host_port.split(']').next().map(|h| &host_port[..h.len() + 1]).unwrap_or(host_port)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    (!host.is_empty()).then_some(host)
}

/// A web tab's title (spec 3.1): the page's title while it has one, else the host, a local
/// page's file name, or "New tab" for a blank one.
pub fn title_for(page_title: Option<&str>, url: &str) -> String {
    if let Some(t) = page_title.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if url.is_empty() {
        return "New tab".to_string();
    }
    if let Some(path) = file_path(url) {
        return path.file_name().map_or_else(|| "page".to_string(), |n| n.to_string_lossy().into_owned());
    }
    url_host(url).unwrap_or("page").to_string()
}

/// The scheme of a URL, for the counters: `http`, `https`, `file` or `other`.
pub fn scheme_of(url: &str) -> &'static str {
    match url.split_once("://").map(|(s, _)| s) {
        Some("http") => "http",
        Some("https") => "https",
        Some("file") => "file",
        _ => "other",
    }
}

/// Where a tab is with its page.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadState {
    /// No page asked for (an empty URL).
    Blank,
    /// A load is under way, this far along (0 to 1).
    Loading { progress: f64 },
    Ready,
    /// The last load failed, for this reason; the bar offers Retry.
    Failed { reason: String },
}

/// What the real view's delegates saw since the last poll (spec 5.4).
#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// A navigation committed to this URL: the tab keeps it.
    Committed(String),
    /// A load failed, with WebKit's words.
    Failed(String),
    /// A load was cancelled (a new address typed, a redirect): not a failure.
    Cancelled,
    /// The page's content process died.
    Terminated,
    /// The page asked for an address Arkos does not show (`mailto:` and the like); it was handed
    /// to the system.
    OpenOutside(String),
    /// The response was something WebKit cannot show (a download).
    Undisplayable,
}

/// What a poll of the view reports.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Poll {
    pub loading: bool,
    pub progress: f64,
    /// The view's URL, when it has one.
    pub url: Option<String>,
    pub title: Option<String>,
    pub can_back: bool,
    pub can_forward: bool,
    pub signals: Vec<Signal>,
}

/// What the model needs from a web view; `wk_web_view::Host` implements it.
pub trait Web {
    /// Loads `url`; `read_root` is the folder a `file` URL may read.
    fn load(&mut self, url: &str, read_root: Option<&Path>);
    fn reload(&mut self);
    fn stop(&mut self);
    fn back(&mut self);
    fn forward(&mut self);
    fn set_zoom(&mut self, zoom: f64);
    /// Shows the view at `frame`, or hides it with `None`.
    fn place(&mut self, frame: Option<Frame>);
    /// What happened since the last poll, and the view's state now.
    fn poll(&mut self) -> Poll;
    /// The colour behind a page that has none, and before the first paint.
    fn set_under_color(&mut self, rgb: (u8, u8, u8));
    /// The page has the keyboard (the window's first responder is inside the view).
    fn has_keys(&self) -> bool {
        false
    }
}

/// A web view that is not there: the window has no `NSWindow` to host one (never in the app;
/// the shoot's headless cases and a view made before the window handle is known).
pub struct NoWeb;

impl Web for NoWeb {
    fn load(&mut self, _url: &str, _read_root: Option<&Path>) {}
    fn reload(&mut self) {}
    fn stop(&mut self) {}
    fn back(&mut self) {}
    fn forward(&mut self) {}
    fn set_zoom(&mut self, _zoom: f64) {}
    fn place(&mut self, _frame: Option<Frame>) {}
    fn poll(&mut self) -> Poll {
        Poll { signals: vec![Signal::Failed("no window to host a page".into())], ..Poll::default() }
    }
    fn set_under_color(&mut self, _rgb: (u8, u8, u8)) {}
}

/// A web view for tests: records its calls as strings and hands back `pending` from `poll`.
#[cfg(test)]
#[derive(Default)]
pub struct FakeWeb {
    pub calls: Vec<String>,
    pub pending: Vec<Poll>,
    pub keys: bool,
}

#[cfg(test)]
impl Web for FakeWeb {
    fn load(&mut self, url: &str, read_root: Option<&Path>) {
        self.calls.push(match read_root {
            Some(r) => format!("load {url} reading {}", r.display()),
            None => format!("load {url}"),
        });
    }
    fn reload(&mut self) {
        self.calls.push("reload".into());
    }
    fn stop(&mut self) {
        self.calls.push("stop".into());
    }
    fn back(&mut self) {
        self.calls.push("back".into());
    }
    fn forward(&mut self) {
        self.calls.push("forward".into());
    }
    fn set_zoom(&mut self, zoom: f64) {
        self.calls.push(format!("zoom {zoom}"));
    }
    fn place(&mut self, frame: Option<Frame>) {
        self.calls.push(match frame {
            Some(f) => format!("place {} {} {} {}", f.x, f.y, f.w, f.h),
            None => "hide".into(),
        });
    }
    fn poll(&mut self) -> Poll {
        if self.pending.is_empty() {
            Poll::default()
        } else {
            self.pending.remove(0)
        }
    }
    fn set_under_color(&mut self, rgb: (u8, u8, u8)) {
        self.calls.push(format!("under {:?}", rgb));
    }
    fn has_keys(&self) -> bool {
        self.keys
    }
}

/// What the model tells its owner after a poll.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelEvent {
    /// The tab's URL changed: write it into the tree and save.
    Save(String),
    /// A line for the bar, for `NOTICE_FOR`.
    Notice(String),
    /// A load ended, for the counters.
    Loaded { ok: bool, scheme: &'static str },
}

/// One web tab's state.
pub struct WebModel {
    /// The page the tab is on (committed), what the workspace keeps. Empty: blank.
    pub url: String,
    /// The address asked for and not yet committed, what the field shows while loading.
    pub requested: Option<String>,
    pub state: LoadState,
    pub title: Option<String>,
    pub can_back: bool,
    pub can_forward: bool,
    pub zoom: f64,
    /// The folder a local page in it may read.
    pub project: PathBuf,
    /// A local page's file as loaded, and one poll ago (`follow`); `None` for a remote page.
    pub shown_fp: Fp,
    pub prev_fp: Fp,
    /// A local page's file is gone; the page stays until it is back.
    pub file_waiting: bool,
    /// When the page's process last crashed, for `CRASH_WINDOW`.
    last_crash: Option<Instant>,
    /// A page has been shown in this view, so a cancelled or failed load leaves it on screen.
    had_page: bool,
}

impl WebModel {
    pub fn new(url: String, project: PathBuf) -> Self {
        WebModel {
            url,
            requested: None,
            state: LoadState::Blank,
            title: None,
            can_back: false,
            can_forward: false,
            zoom: 1.0,
            project,
            shown_fp: None,
            prev_fp: None,
            file_waiting: false,
            last_crash: None,
            had_page: false,
        }
    }

    /// Starts the tab's page, if it has one.
    pub fn open(&mut self, w: &mut dyn Web) {
        if !self.url.is_empty() {
            let url = self.url.clone();
            self.load(w, url);
        }
    }

    /// Goes to `url`.
    pub fn navigate(&mut self, w: &mut dyn Web, url: String) {
        self.load(w, url);
    }

    fn load(&mut self, w: &mut dyn Web, url: String) {
        let root = file_path(&url).map(|p| read_access_root(&p, &self.project));
        w.load(&url, root.as_deref());
        self.requested = Some(url);
        self.state = LoadState::Loading { progress: 0.0 };
        self.shown_fp = None;
        self.prev_fp = None;
        self.file_waiting = false;
    }

    /// Reload, or Retry on the failed screen: a failed load is asked for again from its URL (the
    /// view may hold nothing to reload), a page reloads in place.
    pub fn reload(&mut self, w: &mut dyn Web) {
        match &self.state {
            LoadState::Failed { .. } | LoadState::Blank => {
                let url = self.requested.clone().unwrap_or_else(|| self.url.clone());
                if !url.is_empty() {
                    self.load(w, url);
                }
            }
            LoadState::Loading { .. } | LoadState::Ready => {
                self.state = LoadState::Loading { progress: 0.0 };
                w.reload();
            }
        }
    }

    pub fn stop(&mut self, w: &mut dyn Web) {
        w.stop();
    }

    pub fn zoom_by(&mut self, w: &mut dyn Web, dir: i32) {
        self.zoom = if dir == 0 { 1.0 } else { zoom_step(self.zoom, dir) };
        w.set_zoom(self.zoom);
    }

    /// The address the bar shows: what is being loaded, else the page.
    pub fn shown_address(&self) -> &str {
        match (&self.state, &self.requested) {
            (LoadState::Loading { .. } | LoadState::Failed { .. }, Some(r)) => r,
            _ => &self.url,
        }
    }

    pub fn loading(&self) -> bool {
        matches!(self.state, LoadState::Loading { .. })
    }

    /// Takes what the view reports (spec 5.4).
    pub fn apply(&mut self, w: &mut dyn Web, now: Instant) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        let poll = w.poll();
        self.can_back = poll.can_back;
        self.can_forward = poll.can_forward;
        if poll.title.is_some() {
            self.title = poll.title.clone();
        }
        for signal in poll.signals {
            match signal {
                Signal::Committed(url) => {
                    if url != self.url {
                        self.url = url.clone();
                        events.push(ModelEvent::Save(url));
                    }
                    self.requested = None;
                    self.title = None;
                    self.had_page = true;
                    if !self.loading() {
                        self.state = LoadState::Loading { progress: poll.progress };
                    }
                }
                Signal::Failed(reason) => {
                    let scheme = scheme_of(self.shown_address());
                    self.state = LoadState::Failed { reason };
                    events.push(ModelEvent::Loaded { ok: false, scheme });
                }
                Signal::Cancelled => {
                    if self.loading() && !poll.loading {
                        self.state = if self.had_page { LoadState::Ready } else { LoadState::Blank };
                        self.requested = None;
                    }
                }
                Signal::Terminated => {
                    let again = self.last_crash.is_some_and(|t| now.saturating_duration_since(t) < CRASH_WINDOW);
                    self.last_crash = Some(now);
                    if again {
                        self.state = LoadState::Failed { reason: "the page's process crashed twice in a minute".into() };
                    } else {
                        let url = self.url.clone();
                        self.load(w, url);
                        events.push(ModelEvent::Notice("the page crashed; loading it again".into()));
                    }
                }
                Signal::OpenOutside(url) => events.push(ModelEvent::Notice(format!("opened outside Arkos: {url}"))),
                Signal::Undisplayable => events.push(ModelEvent::Notice("downloads are not supported here; open in your browser".into())),
            }
        }
        match &mut self.state {
            LoadState::Loading { progress } => {
                if poll.loading {
                    *progress = poll.progress.max(*progress);
                } else if poll.progress >= 1.0 || poll.url.is_some() {
                    self.state = LoadState::Ready;
                    self.requested = None;
                    self.had_page = true;
                    events.push(ModelEvent::Loaded { ok: true, scheme: scheme_of(&self.url) });
                }
            }
            LoadState::Blank | LoadState::Ready | LoadState::Failed { .. } => {}
        }
        events
    }

    /// The once a second poll of a local page's file (spec 6): `fp` is its fingerprint now, or
    /// `None` for a remote page, which is never polled. True when a reload started.
    pub fn tick_file(&mut self, w: &mut dyn Web, fp: Fp) -> bool {
        if file_path(&self.url).is_none() || self.loading() {
            return false;
        }
        if self.shown_fp.is_none() && !self.file_waiting {
            // The page just committed: this is the file as loaded.
            self.shown_fp = fp;
            self.prev_fp = fp;
            return false;
        }
        let decision = follow(self.shown_fp, self.prev_fp, fp, self.file_waiting);
        self.prev_fp = fp;
        match decision {
            Follow::Reload | Follow::Back => {
                self.shown_fp = fp;
                self.file_waiting = false;
                self.state = LoadState::Loading { progress: 0.0 };
                w.reload();
                true
            }
            Follow::Waiting => {
                self.file_waiting = true;
                false
            }
            Follow::Nothing => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "<html></html>").unwrap();
        p.canonicalize().unwrap()
    }

    #[test]
    fn an_address_keeps_its_scheme_and_a_bare_host_gets_one() {
        let cwd = Path::new("/");
        assert_eq!(parse_address("https://example.com/x", cwd).unwrap(), "https://example.com/x");
        assert_eq!(parse_address(" http://x.dev ", cwd).unwrap(), "http://x.dev");
        assert_eq!(parse_address("example.com", cwd).unwrap(), "https://example.com");
        assert_eq!(parse_address("docs.rs/gpui", cwd).unwrap(), "https://docs.rs/gpui");
        assert_eq!(parse_address("localhost", cwd).unwrap(), "http://localhost");
        assert_eq!(parse_address("localhost:4173/", cwd).unwrap(), "http://localhost:4173/");
        assert_eq!(parse_address("127.0.0.1:8080", cwd).unwrap(), "http://127.0.0.1:8080");
        assert_eq!(parse_address("[::1]:3000/x", cwd).unwrap(), "http://[::1]:3000/x");
        assert_eq!(parse_address("mybox.local", cwd).unwrap(), "http://mybox.local");
        assert_eq!(parse_address("devbox:8000", cwd).unwrap(), "http://devbox:8000", "a host with a port is this network's");
    }

    #[test]
    fn what_is_not_an_address_is_refused_not_searched() {
        let cwd = Path::new("/");
        assert!(parse_address("", cwd).is_err());
        assert!(parse_address("hello world", cwd).unwrap_err().starts_with("not an address"));
        assert!(parse_address("gpui", cwd).unwrap_err().starts_with("not an address"));
        assert!(parse_address("example.com:abc", cwd).is_err());
        assert_eq!(parse_address("mailto:me@x.dev", cwd).unwrap_err(), "mailto: addresses open outside Arkos");
        assert!(parse_address("vscode://file/x", cwd).is_err());
    }

    #[test]
    fn a_path_becomes_a_file_url_when_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let page = tmp_file(dir.path(), "my page.html");
        let url = parse_address(page.to_str().unwrap(), Path::new("/")).unwrap();
        assert!(url.starts_with("file:///") && url.ends_with("/my%20page.html"), "{url}");
        assert_eq!(file_path(&url).unwrap(), page, "the URL reads back as the path");
        let cwd = dir.path().canonicalize().unwrap();
        assert_eq!(parse_address("my page.html", &cwd).unwrap(), url, "a relative .html is the shell's folder's");
        assert_eq!(parse_address("./my page.html", &cwd).unwrap(), url);
        assert!(parse_address("missing.html", &cwd).unwrap_err().starts_with("no such file"));
        assert!(parse_address(dir.path().to_str().unwrap(), &cwd).unwrap_err().ends_with("is a folder"));
        assert_eq!(parse_address("file:///tmp/x.html", &cwd).unwrap(), "file:///tmp/x.html", "a file URL is taken as given");
    }

    #[test]
    fn a_local_page_reads_its_project_or_its_own_folder() {
        let project = Path::new("/p/site");
        assert_eq!(read_access_root(Path::new("/p/site/out/report.html"), project), PathBuf::from("/p/site"));
        assert_eq!(read_access_root(Path::new("/tmp/x/report.html"), project), PathBuf::from("/tmp/x"));
        assert_eq!(read_access_root(Path::new("/p/sitecopy/a.html"), project), PathBuf::from("/p/sitecopy"), "a sibling with the prefix is outside");
    }

    #[test]
    fn a_tab_is_titled_by_its_page_else_its_host_file_or_new_tab() {
        assert_eq!(title_for(Some("Docs - Example"), "https://example.com/docs"), "Docs - Example");
        assert_eq!(title_for(Some("  "), "https://example.com/docs"), "example.com", "a blank title falls back");
        assert_eq!(title_for(None, "http://me@localhost:3000/x"), "localhost");
        assert_eq!(title_for(None, "https://[::1]:8080/"), "[::1]");
        assert_eq!(title_for(None, "file:///Users/me/out/my%20report.html"), "my report.html");
        assert_eq!(title_for(None, "about:blank"), "page");
        assert_eq!(title_for(None, ""), "New tab");
        assert_eq!(scheme_of("file:///x"), "file");
        assert_eq!(scheme_of("about:blank"), "other");
    }

    #[test]
    fn zoom_walks_the_ladder_and_holds_at_its_ends() {
        assert_eq!(zoom_step(1.0, 1), 1.1);
        assert_eq!(zoom_step(1.0, -1), 0.9);
        assert_eq!(zoom_step(2.0, 1), 2.0);
        assert_eq!(zoom_step(0.5, -1), 0.5);
        assert_eq!(zoom_step(1.3, 1), 1.5, "off the ladder, from the nearest rung");
    }

    fn ready(url: &str, title: &str) -> Poll {
        Poll { loading: false, progress: 1.0, url: Some(url.into()), title: Some(title.into()), ..Poll::default() }
    }

    #[test]
    fn a_blank_tab_loads_nothing_and_a_page_loads_with_progress_then_a_title() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new(String::new(), "/p".into());
        m.open(&mut w);
        assert!(w.calls.is_empty());
        assert_eq!(m.state, LoadState::Blank);
        let now = Instant::now();
        m.navigate(&mut w, "https://example.com/".into());
        assert_eq!(w.calls, ["load https://example.com/"]);
        assert_eq!(m.state, LoadState::Loading { progress: 0.0 });
        assert_eq!(m.shown_address(), "https://example.com/", "the field shows what is being loaded");
        w.pending.push(Poll { loading: true, progress: 0.4, signals: vec![Signal::Committed("https://example.com/".into())], ..Poll::default() });
        let events = m.apply(&mut w, now);
        assert_eq!(events, [ModelEvent::Save("https://example.com/".into())]);
        assert_eq!(m.state, LoadState::Loading { progress: 0.4 });
        assert_eq!(m.url, "https://example.com/");
        w.pending.push(ready("https://example.com/", "Example Domain"));
        let events = m.apply(&mut w, now);
        assert_eq!(events, [ModelEvent::Loaded { ok: true, scheme: "https" }]);
        assert_eq!(m.state, LoadState::Ready);
        assert_eq!(m.title.as_deref(), Some("Example Domain"));
        assert_eq!(title_for(m.title.as_deref(), &m.url), "Example Domain");
    }

    #[test]
    fn a_failure_keeps_the_address_and_the_reason_and_retry_loads_it_again() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("http://localhost:1/".into(), "/p".into());
        m.open(&mut w);
        w.pending.push(Poll { signals: vec![Signal::Failed("Could not connect to the server.".into())], ..Poll::default() });
        let events = m.apply(&mut w, Instant::now());
        assert_eq!(m.state, LoadState::Failed { reason: "Could not connect to the server.".into() });
        assert_eq!(events, [ModelEvent::Loaded { ok: false, scheme: "http" }]);
        assert_eq!(m.shown_address(), "http://localhost:1/");
        m.reload(&mut w);
        assert_eq!(w.calls, ["load http://localhost:1/", "load http://localhost:1/"], "a retry asks for the address again, not a reload of nothing");
        assert_eq!(m.state, LoadState::Loading { progress: 0.0 });
    }

    #[test]
    fn a_cancelled_navigation_is_not_a_failure() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("https://a.dev/".into(), "/p".into());
        m.open(&mut w);
        w.pending.push(Poll { signals: vec![Signal::Committed("https://a.dev/".into())], loading: true, ..Poll::default() });
        m.apply(&mut w, Instant::now());
        w.pending.push(ready("https://a.dev/", "A"));
        m.apply(&mut w, Instant::now());
        m.navigate(&mut w, "https://b.dev/".into());
        w.pending.push(Poll { loading: false, signals: vec![Signal::Cancelled], url: Some("https://a.dev/".into()), ..Poll::default() });
        let events = m.apply(&mut w, Instant::now());
        assert_eq!(m.state, LoadState::Ready, "the old page is still there");
        assert_eq!(m.url, "https://a.dev/");
        assert_eq!(m.shown_address(), "https://a.dev/", "the field goes back to the page");
        assert!(events.is_empty());
    }

    #[test]
    fn a_crash_reloads_once_then_fails() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("https://a.dev/".into(), "/p".into());
        m.open(&mut w);
        let t0 = Instant::now();
        w.pending.push(Poll { signals: vec![Signal::Terminated], ..Poll::default() });
        let events = m.apply(&mut w, t0);
        assert_eq!(events, [ModelEvent::Notice("the page crashed; loading it again".into())]);
        assert_eq!(w.calls, ["load https://a.dev/", "load https://a.dev/"]);
        w.pending.push(Poll { signals: vec![Signal::Terminated], ..Poll::default() });
        m.apply(&mut w, t0 + Duration::from_secs(10));
        assert_eq!(m.state, LoadState::Failed { reason: "the page's process crashed twice in a minute".into() });
        assert_eq!(w.calls.len(), 2, "no third load");
    }

    #[test]
    fn a_local_page_reloads_when_its_file_changed_and_held_still_and_waits_while_it_is_gone() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("file:///p/out/report.html".into(), "/p".into());
        m.open(&mut w);
        assert_eq!(w.calls, ["load file:///p/out/report.html reading /p"]);
        w.pending.push(ready("file:///p/out/report.html", "Report"));
        m.apply(&mut w, Instant::now());
        let (a, b) = (Some((1, 10)), Some((2, 12)));
        assert!(!m.tick_file(&mut w, a), "the first poll after the load is the file as shown");
        assert!(!m.tick_file(&mut w, a));
        assert!(!m.tick_file(&mut w, b), "changed: wait a poll");
        assert!(m.tick_file(&mut w, b), "still: reload");
        assert_eq!(w.calls.last().unwrap(), "reload");
        assert_eq!(m.state, LoadState::Loading { progress: 0.0 });
        w.pending.push(ready("file:///p/out/report.html", "Report"));
        m.apply(&mut w, Instant::now());
        assert!(!m.tick_file(&mut w, None), "gone: the page stays");
        assert!(m.file_waiting);
        assert!(!m.tick_file(&mut w, b), "back, but the first sight is not yet stable");
        assert!(m.tick_file(&mut w, b), "back and still: reload");
    }

    #[test]
    fn a_remote_page_is_never_polled() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("https://a.dev/".into(), "/p".into());
        m.open(&mut w);
        w.pending.push(ready("https://a.dev/", "A"));
        m.apply(&mut w, Instant::now());
        assert!(!m.tick_file(&mut w, None));
        assert!(!m.tick_file(&mut w, Some((1, 1))));
        assert_eq!(w.calls, ["load https://a.dev/"]);
    }

    #[test]
    fn a_notice_comes_for_a_download_and_an_outside_address() {
        let mut w = FakeWeb::default();
        let mut m = WebModel::new("https://a.dev/".into(), "/p".into());
        m.open(&mut w);
        w.pending.push(Poll { signals: vec![Signal::Undisplayable, Signal::OpenOutside("mailto:x@y".into())], ..Poll::default() });
        let events = m.apply(&mut w, Instant::now());
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ModelEvent::Notice(n) if n.starts_with("downloads are not supported")));
        assert!(matches!(&events[1], ModelEvent::Notice(n) if n.contains("mailto:x@y")));
    }
}
