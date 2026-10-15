//! A read-only viewer tab (spec 6): the file loaded once off the UI thread, classified (text,
//! too big to highlight, binary), split into lines and highlighted; a find over the lines; a
//! scroll position in lines (in display rows while the inline diff shows deleted lines);
//! reloaded when the poll sees the file change. A file in a repository carries its diff
//! against HEAD (`diff`), recomputed on every load and on the ten-second timer.

use crate::config_watch::fingerprint;
use crate::diff::{self, DiffMarks};
use crate::find::{FindLook, BAR_TOP};
use crate::highlight::{self, Highlighter};
use crate::line_edit::{self, Edit, LineEdit};
use crate::motion::{self, Fades};
use crate::palette::Rgb;
use crate::scroll::ScrollAnimator;
use crate::theme::Theme;
use crate::tree::{PaneId, Tab, TabId};
use crate::viewer;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, Entity, EntityId, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, SharedString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Bytes a binary check looks at: a NUL in the first 8 KiB means not text.
pub const SNIFF: usize = 8 * 1024;
pub const MAX_HITS: usize = 10_000;
/// Files above this are not read at all: a notice says how big they are.
pub const MAX_PLAIN_BYTES: u64 = 64 << 20;
/// A file with a line longer than this (a minified bundle, a one-line JSON dump) is shown
/// plain: the grammars would chew on the whole line at once.
pub const MAX_LINE_BYTES: usize = 8 * 1024;

/// Why a text file is shown without colours, under a banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plain {
    /// Over the configured highlight limit.
    TooBig,
    /// A line over `MAX_LINE_BYTES`.
    LongLine,
}

impl Plain {
    pub fn banner(self) -> &'static str {
        match self {
            Plain::TooBig => "over the highlight limit, shown plain",
            Plain::LongLine => "a line is longer than 8 KB, shown plain",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line.
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// The lines are shared with a find running on the background executor.
    Text { lines: Arc<Vec<String>>, spans: Vec<Vec<Span>> },
    /// Over `MAX_PLAIN_BYTES`: the size, the file unread.
    TooLarge(u64),
    Binary,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    Text(String),
    TooBig,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

pub fn classify(bytes: &[u8], limit: usize) -> Classified {
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return Classified::Binary;
    }
    if bytes.len() > limit {
        return Classified::TooBig;
    }
    Classified::Text(String::from_utf8_lossy(bytes).into_owned())
}

/// Lines without their terminators; a trailing newline adds no empty last line. A tab becomes
/// four spaces (a shaped line draws it as one glyph) and a lone carriage return goes.
pub fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines().map(|l| if l.contains(['\t', '\r']) { l.replace('\r', "").replace('\t', "    ") } else { l.to_string() }).collect()
}

/// Every case-insensitive occurrence, in reading order, as byte ranges, at most `MAX_HITS`.
pub fn find_hits(lines: &[String], needle: &str) -> Vec<Hit> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let n: Vec<char> = needle.to_lowercase().chars().collect();
    let ascii = needle.is_ascii().then(|| needle.as_bytes());
    for (i, line) in lines.iter().enumerate() {
        // The common case, an ASCII line and needle: compare bytes, no allocation.
        if let Some(nb) = ascii.filter(|_| line.is_ascii()) {
            let hay = line.as_bytes();
            let mut b = 0;
            while b + nb.len() <= hay.len() {
                if hay[b..b + nb.len()].eq_ignore_ascii_case(nb) {
                    out.push(Hit { line: i, start: b, end: b + nb.len() });
                    if out.len() >= MAX_HITS {
                        return out;
                    }
                    b += nb.len();
                } else {
                    b += 1;
                }
            }
            continue;
        }
        // Lowercasing can change byte lengths (e.g. some scripts): search char by char instead.
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut c = 0;
        while c + n.len() <= chars.len() {
            let matches = (0..n.len()).all(|k| chars[c + k].1.to_lowercase().eq(n[k].to_lowercase()));
            if matches {
                let start = chars[c].0;
                let end = chars.get(c + n.len()).map(|(b, _)| *b).unwrap_or(line.len());
                out.push(Hit { line: i, start, end });
                if out.len() >= MAX_HITS {
                    return out;
                }
                c += n.len();
            } else {
                c += 1;
            }
        }
    }
    out
}

/// The next hit index, wrapping.
pub fn step(current: usize, len: usize, delta: isize) -> usize {
    crate::keymap::tab_cycle(current, len, delta)
}

pub struct ViewerFind {
    pub query: LineEdit,
    pub hits: Vec<Hit>,
    pub current: usize,
    pub focus: FocusHandle,
    /// A search is running on the background executor; `hits` are the previous ones.
    pub searching: bool,
}

/// What the background load produced, applied on the UI thread.
pub struct Loaded {
    pub content: Content,
    pub fingerprint: Option<(u128, u64)>,
    /// Shown without colours, under a banner saying why.
    pub plain: Option<Plain>,
    /// The changes against HEAD; `None` outside a repository, for a file HEAD lacks, a clean
    /// file, or one shown plain for its size.
    pub diff: Option<DiffMarks>,
}

pub struct Viewer {
    pub path: PathBuf,
    pub content: Content,
    /// `false` until the first load lands.
    pub loaded: bool,
    pub scroll: ScrollAnimator,
    pub focus: FocusHandle,
    pub find: Option<ViewerFind>,
    pub font_size: f32,
    pub line_height: f32,
    pub last_frame: Option<Instant>,
    /// Rows the element showed at its last paint, for page keys and the scroll limit.
    pub rows_visible: usize,
    pub fingerprint: Option<(u128, u64)>,
    /// The file is shown plain, and why.
    pub plain: Option<Plain>,
    /// Numbers every load started; `applied` is the newest that landed, so a slower, older
    /// load never lands over a newer one.
    pub loads: u64,
    pub applied: u64,
    /// Numbers every search started; only the latest one's hits land.
    pub find_seq: u64,
    /// The changes against HEAD, from the last load or rediff; `None` when there are none.
    pub diff: Option<DiffMarks>,
    /// The deleted lines are shown between the current ones; the scroll is then in display rows.
    pub inline_diff: bool,
}

impl Viewer {
    pub fn new(path: PathBuf, font_size: f32, line_height: f32, cx: &mut Context<Self>) -> Self {
        Viewer {
            path,
            content: Content::Text { lines: Arc::new(Vec::new()), spans: Vec::new() },
            loaded: false,
            scroll: ScrollAnimator::new(),
            focus: cx.focus_handle(),
            find: None,
            font_size,
            line_height,
            last_frame: None,
            rows_visible: 1,
            fingerprint: None,
            plain: None,
            loads: 0,
            applied: 0,
            find_seq: 0,
            diff: None,
            inline_diff: false,
        }
    }

    pub fn line_count(&self) -> usize {
        match &self.content {
            Content::Text { lines, .. } => lines.len(),
            _ => 0,
        }
    }

    /// The rows the element paints: the lines, plus the deleted ones with the inline diff on.
    pub fn row_count(&self) -> usize {
        diff::row_count(self.line_count(), self.diff.as_ref(), self.inline_diff)
    }

    /// The furthest the view can scroll: the last row at the top of the viewport.
    pub fn max_scroll(&self) -> f32 {
        self.row_count().saturating_sub(1) as f32
    }

    /// The display row `line` is painted on.
    pub fn row_of_line(&self, line: usize) -> usize {
        match &self.diff {
            Some(d) if self.inline_diff => diff::row_of_line(d, line),
            _ => line,
        }
    }

    /// Shows or hides the deleted lines between the current ones, when there are changes. The
    /// line at the top stays at the top; a view at the very top stays there, so lines deleted
    /// above the first one show.
    pub fn toggle_inline_diff(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.diff.as_ref().filter(|d| d.has_changes()) else { return };
        let (top, frac) = (self.scroll.current.floor().max(0.0), self.scroll.current - self.scroll.current.floor());
        let line = if self.inline_diff { diff::line_at_row(d, &diff::display_rows(self.line_count(), d, true), top as usize) } else { top as usize };
        self.inline_diff = !self.inline_diff;
        let row = if self.scroll.current <= 0.0 { 0.0 } else { self.row_of_line(line) as f32 + frac };
        self.scroll.jump_to(row.min(self.max_scroll()));
        cx.notify();
    }

    /// A rediff landed: new marks for the same lines. Rows come and go with the inline diff on,
    /// so the scroll is clamped again.
    pub fn set_diff(&mut self, diff: Option<DiffMarks>, cx: &mut Context<Self>) {
        if self.diff == diff {
            return;
        }
        self.diff = diff;
        self.scroll.clamp_max(self.max_scroll());
        cx.notify();
    }

    pub fn apply(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.content = loaded.content;
        self.fingerprint = loaded.fingerprint;
        self.plain = loaded.plain;
        self.diff = loaded.diff;
        self.loaded = true;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            // The old hits point into the old lines.
            f.hits.clear();
        }
        self.search(false, cx);
        cx.notify();
    }

    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.scroll.scroll_by(lines, self.max_scroll());
        cx.notify();
    }

    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(ViewerFind { query: LineEdit::new(), hits: Vec::new(), current: 0, focus: cx.focus_handle(), searching: false });
        }
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        // A search still running lands in no later bar.
        self.find_seq += 1;
        cx.notify();
    }

    /// Re-runs the search after the query changed and jumps to the first hit.
    pub fn refind(&mut self, cx: &mut Context<Self>) {
        self.search(true, cx);
        cx.notify();
    }

    /// Searches the lines for the query on the background executor. The result lands only if no
    /// newer search started; with `jump` it points at the first hit and scrolls to it, else the
    /// position is kept (a reload).
    fn search(&mut self, jump: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        self.find_seq += 1;
        let seq = self.find_seq;
        let lines = match &self.content {
            Content::Text { lines, .. } if !f.query.text.is_empty() => lines.clone(),
            _ => {
                f.hits.clear();
                f.current = 0;
                f.searching = false;
                return;
            }
        };
        f.searching = true;
        let query = f.query.text.clone();
        cx.spawn(async move |this, cx| {
            let hits = cx.background_executor().spawn(async move { find_hits(&lines, &query) }).await;
            let _ = this.update(cx, |v, cx| {
                // Every search before the latest one started is superseded.
                if !should_apply(seq, v.find_seq.saturating_sub(1)) {
                    return;
                }
                let Some(f) = &mut v.find else { return };
                f.current = if jump { 0 } else { f.current.min(hits.len().saturating_sub(1)) };
                f.hits = hits;
                f.searching = false;
                if jump {
                    v.show_current_hit();
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.find {
            f.current = step(f.current, f.hits.len(), delta);
        }
        self.show_current_hit();
        cx.notify();
    }

    /// Keys typed into the find bar: Escape closes, Enter and Shift+Enter (and Down and Up)
    /// step, the rest edits the query and searches again.
    pub fn find_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        match f.query.handle(ks) {
            Edit::Changed => self.refind(cx),
            Edit::Submit | Edit::Down => self.find_step(1, cx),
            Edit::SubmitBack | Edit::Up => self.find_step(-1, cx),
            Edit::Cancel => self.close_find(cx),
            // A cursor move changes only the bar.
            Edit::Unchanged => cx.notify(),
        }
    }

    pub fn find_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.find {
            f.query.insert_str(text);
        }
        self.refind(cx);
    }

    /// Scrolls so the current hit's line is in view, a third of the way down.
    fn show_current_hit(&mut self) {
        let Some(f) = &self.find else { return };
        let Some(h) = f.hits.get(f.current) else { return };
        let target = (self.row_of_line(h.line) as f32 - (self.rows_visible as f32 / 3.0).floor()).max(0.0);
        self.scroll.target = target.min(self.max_scroll());
    }
}

/// A viewer's file, read and highlighted: call it on the background executor. `None` when the
/// fingerprint is still `known` (nothing to reload). Over `limit`, or with a line over
/// `MAX_LINE_BYTES`, the text is shown plain; binaries and unreadable files say so. With
/// `with_diff`, text not shown plain for its size is diffed against its worktree's HEAD.
pub fn read_file(path: &Path, known: Option<(u128, u64)>, highlighter: Option<&Highlighter>, limit: usize, with_diff: bool) -> Option<Loaded> {
    let fingerprint = fingerprint(path);
    if known.is_some() && fingerprint == known {
        return None;
    }
    let plain = |lines: Vec<String>| {
        let spans = vec![Vec::new(); lines.len()];
        Content::Text { lines: Arc::new(lines), spans }
    };
    let (content, why_plain) = match read_capped(path) {
        Err(e) => (Content::Unreadable(e.to_string()), None),
        Ok(Capped::TooLarge(len)) => (Content::TooLarge(len), None),
        Ok(Capped::Binary) => (Content::Binary, None),
        Ok(Capped::Bytes(bytes)) => match classify(&bytes, limit) {
            Classified::Binary => (Content::Binary, None),
            Classified::TooBig => (plain(split_lines(&String::from_utf8_lossy(&bytes))), Some(Plain::TooBig)),
            Classified::Text(text) => {
                let lines = split_lines(&text);
                match highlighter {
                    _ if lines.iter().any(|l| l.len() > MAX_LINE_BYTES) => (plain(lines), Some(Plain::LongLine)),
                    Some(h) => {
                        let spans = h.highlight(path, &lines);
                        (Content::Text { lines: Arc::new(lines), spans }, None)
                    }
                    // The grammars are still loading: plain for now, coloured when they land.
                    None => (plain(lines), None),
                }
            }
        },
    };
    let diff = match &content {
        Content::Text { lines, .. } if with_diff && why_plain != Some(Plain::TooBig) => diff_of(path, lines),
        _ => None,
    };
    Some(Loaded { content, fingerprint, plain: why_plain, diff })
}

/// The file's changes against its worktree's HEAD, or `None` when it has no base or no changes.
/// Spawns git: background executor only.
pub fn diff_of(path: &Path, lines: &[String]) -> Option<DiffMarks> {
    let base = diff::base_for(path)?;
    Some(diff::compute(&base, lines)).filter(DiffMarks::has_changes)
}

enum Capped {
    TooLarge(u64),
    Binary,
    Bytes(Vec<u8>),
}

/// The file's bytes, unless it is over `MAX_PLAIN_BYTES` (not opened) or has a NUL in its first
/// `SNIFF` bytes (nothing more read). A directory is refused before anything is opened.
fn read_capped(path: &Path) -> std::io::Result<Capped> {
    let metadata = std::fs::metadata(path)?;
    if metadata.is_dir() {
        return Err(std::io::Error::other("is a directory"));
    }
    let len = metadata.len();
    if len > MAX_PLAIN_BYTES {
        return Ok(Capped::TooLarge(len));
    }
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(len as usize);
    (&mut file).take(SNIFF as u64).read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Ok(Capped::Binary);
    }
    // Capped again here: the file may have grown since the metadata was read.
    file.take(MAX_PLAIN_BYTES - bytes.len() as u64).read_to_end(&mut bytes)?;
    Ok(Capped::Bytes(bytes))
}

pub fn count_text(current: usize, len: usize) -> String {
    match len {
        0 => "no matches".to_string(),
        // The search stops at MAX_HITS; there may be more.
        _ => format!("{} of {len}{}", current + 1, if len >= MAX_HITS { "+" } else { "" }),
    }
}

/// The viewer's find bar, `find::render`'s chrome: the query, the count, a close button. Keys
/// go to `find_key`, Cmd+V pastes; a click focuses `pane`. It fades and slides like the
/// terminal's; while closing it is only drawn.
pub fn find_bar(viewer: &Entity<Viewer>, pane: PaneId, theme: &Theme, family: SharedString, bars: &mut Fades<EntityId, FindLook>, now: Instant, cx: &mut Context<Root>) -> Option<Div> {
    let (look, focus) = match viewer.read(cx).find.as_ref() {
        Some(f) => {
            let count = if f.searching { "searching".to_string() } else { count_text(f.current, f.hits.len()) };
            (Some(FindLook { query: f.query.clone(), count }), Some(f.focus.clone()))
        }
        None => (None, None),
    };
    let (FindLook { query, count }, opacity) = bars.sync(viewer.entity_id(), look, now).map(|(l, o)| (l.clone(), o))?;
    let live = focus.is_some();
    let (v_keys, v_close) = (viewer.clone(), viewer.clone());
    Some(
        div()
            .absolute()
            .top(px(BAR_TOP - motion::lift(opacity, motion::PANEL_LIFT)))
            .opacity(opacity)
            .right(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .text_color(theme.chrome.text.hsla())
            .font_family(family.clone())
            .text_size(px(12.0))
            .when_some(focus, |d, focus| {
                d.track_focus(&focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                            root.focus_pane(pane, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
                        // The viewer's own key handler would scroll on the same key.
                        cx.stop_propagation();
                        let ks = &ev.keystroke;
                        if ks.modifiers.platform && ks.key == "v" {
                            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                                v_keys.update(cx, |v, cx| v.find_paste(&text, cx));
                            }
                            return;
                        }
                        v_keys.update(cx, |v, cx| v.find_key(ks, cx));
                    }))
            })
            .child(div().w(px(220.0)).child(line_edit::render(&query, "find", theme, family, 12.0)))
            .child(div().text_color(theme.chrome.text_dim.hsla()).child(count))
            .child(
                div()
                    .id(("viewer-find-close", viewer.entity_id()))
                    .text_color(theme.chrome.text_dim.hsla())
                    .child("×")
                    .when(live, |d| {
                        d.hover(|s| s.text_color(theme.chrome.text.hsla()))
                            .cursor_pointer()
                            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v_close.update(cx, |v, cx| v.close_find(cx))))
                    }),
            ),
    )
}

/// Whether a background result numbered `seq` lands over the one numbered `applied`: only a
/// newer one does, so a slow, older result never replaces a newer one.
pub fn should_apply(seq: u64, applied: u64) -> bool {
    seq > applied
}

impl Root {
    /// Loads the grammars off the UI thread, once, at launch. The result takes the theme's
    /// colours at the moment it lands, so a theme change during the load is not lost.
    pub(crate) fn build_highlighter(&mut self, cx: &mut Context<Self>) {
        let (colors, text) = (self.theme.viewer.clone(), self.theme.chrome.text);
        cx.spawn(async move |this, cx| {
            let built = cx.background_executor().spawn(async move { highlight::Highlighter::new(&colors, text) }).await;
            let _ = this.update(cx, |root, cx| {
                root.highlighter = Some(Arc::new(built.with_theme(&root.theme.viewer, root.theme.chrome.text)));
                // Viewers opened before the grammars were ready are plain until now.
                root.reload_all_viewers(cx);
            });
        })
        .detach();
    }

    /// The viewer of a viewer tab, made (and its load started) the first time the tab is shown.
    pub fn viewer_for(&mut self, tab: TabId, path: &Path, cx: &mut Context<Self>) -> Entity<Viewer> {
        if let Some(v) = self.viewers.get(&tab) {
            return v.clone();
        }
        let (font_size, line_height) = (self.font_size, self.line_height);
        let v = cx.new(|cx| Viewer::new(path.to_path_buf(), font_size, line_height, cx));
        self.viewers.insert(tab, v.clone());
        self.load_viewer(tab, cx);
        v
    }

    /// Reads, classifies and highlights a viewer's file on the background executor and applies
    /// the result. A deleted or unreadable file shows why.
    pub fn load_viewer(&mut self, tab: TabId, cx: &mut Context<Self>) {
        self.load(tab, false, cx);
    }

    /// Reloads every viewer showing `path` whose file changed since it was loaded; returns
    /// whether any viewer shows it.
    pub fn reload_viewers_for(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let tabs: Vec<TabId> = self.viewers.iter().filter(|(_, v)| v.read(cx).path == path).map(|(t, _)| *t).collect();
        for &tab in &tabs {
            self.load(tab, true, cx);
        }
        !tabs.is_empty()
    }

    /// Reloads the viewers whose colours depend on the highlighter: plain text, binaries and
    /// files too large to show have nothing to recolour.
    pub(crate) fn reload_all_viewers(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<TabId> = self.viewers.iter().filter(|(_, v)| {
            let v = v.read(cx);
            v.plain.is_none() && matches!(v.content, viewer::Content::Text { .. })
        }).map(|(t, _)| *t).collect();
        for tab in tabs {
            self.load_viewer(tab, cx);
        }
    }

    /// The ten-second timer's look at HEAD: every text viewer's diff computed again against the
    /// lines it shows, so a commit (or a checkout) moves the marks with the file untouched. A
    /// result lands only if no load landed meanwhile; the file is not read again.
    pub(crate) fn rediff_viewers(&mut self, cx: &mut Context<Self>) {
        let viewers: Vec<Entity<Viewer>> = self.viewers.values().cloned().collect();
        for viewer in viewers {
            let (path, lines, applied) = {
                let v = viewer.read(cx);
                match &v.content {
                    Content::Text { lines, .. } if v.loaded && v.plain != Some(Plain::TooBig) => (v.path.clone(), lines.clone(), v.applied),
                    _ => continue,
                }
            };
            let weak = viewer.downgrade();
            cx.spawn(async move |_, cx| {
                let shown = lines.clone();
                let diff = cx.background_executor().spawn(async move { diff_of(&path, &lines) }).await;
                let _ = weak.update(cx, |v, cx| {
                    let same = matches!(&v.content, Content::Text { lines, .. } if Arc::ptr_eq(lines, &shown));
                    if v.applied == applied && same {
                        v.set_diff(diff, cx);
                    }
                });
            })
            .detach();
        }
    }

    /// One background load. With `if_changed`, a file whose fingerprint matches the loaded one
    /// is left alone (the poll's first look at a viewer's file can report a change it already shows).
    fn load(&mut self, tab: TabId, if_changed: bool, cx: &mut Context<Self>) {
        let Some(viewer) = self.viewers.get(&tab).cloned() else { return };
        let (path, known, seq) = viewer.update(cx, |v, _| {
            v.loads += 1;
            let known = if if_changed && v.loaded { v.fingerprint } else { None };
            (v.path.clone(), known, v.loads)
        });
        let highlighter = self.highlighter.clone();
        let limit = self.config.viewer.highlight_limit_bytes;
        let weak = viewer.downgrade();
        cx.spawn(async move |_, cx| {
            let loaded = cx.background_executor().spawn(async move { viewer::read_file(&path, known, highlighter.as_deref(), limit, true) }).await;
            let Some(loaded) = loaded else { return };
            let _ = weak.update(cx, |v, cx| {
                if should_apply(seq, v.applied) {
                    v.applied = seq;
                    v.apply(loaded, cx);
                }
            });
        })
        .detach();
    }

    /// The focused pane's active viewer, once its tab has been shown.
    pub fn active_viewer(&self) -> Option<Entity<Viewer>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Viewer { .. } => self.viewers.get(p.ids.get(p.active)?).cloned(),
            Tab::Terminal { .. } | Tab::Browser { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_text_too_big_and_binary() {
        assert_eq!(classify(b"hello\nworld\n", 100), Classified::Text("hello\nworld\n".into()));
        assert_eq!(classify(b"hello\nworld\n", 5), Classified::TooBig, "over the limit: no highlighting, plain lines");
        assert_eq!(classify(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", 100), Classified::Binary);
        let mut big = vec![b'a'; SNIFF + 10];
        big[SNIFF + 5] = 0;
        assert!(matches!(classify(&big, 1 << 20), Classified::Text(_)), "a NUL past the sniff window is not checked");
        assert!(matches!(classify(&[0xff, 0xfe, b'a'], 100), Classified::Text(_)), "invalid UTF-8 is shown lossily, not called binary");
        assert_eq!(classify(b"", 0), Classified::Text(String::new()), "an empty file is text under any limit");
    }

    #[test]
    fn lines_split_on_lf_and_crlf_without_a_phantom_last_line() {
        assert_eq!(split_lines("a\nb\n"), ["a", "b"]);
        assert_eq!(split_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("\n"), [""]);
        assert_eq!(split_lines("one"), ["one"]);
    }

    #[test]
    fn a_tab_becomes_four_spaces_and_a_lone_carriage_return_goes() {
        assert_eq!(split_lines("a\tb"), ["a    b"]);
        assert_eq!(split_lines("\tx\n"), ["    x"]);
        assert_eq!(split_lines("a\rb\r"), ["ab"], "a lone CR is not drawn");
        assert_eq!(split_lines("a\r\nb\r\n"), ["a", "b"]);
    }

    #[test]
    fn find_is_case_insensitive_in_order_and_capped() {
        let lines: Vec<String> = ["Hello hello", "nothing", "HELLO"].iter().map(|s| s.to_string()).collect();
        let hits = find_hits(&lines, "hello");
        assert_eq!(hits, [Hit { line: 0, start: 0, end: 5 }, Hit { line: 0, start: 6, end: 11 }, Hit { line: 2, start: 0, end: 5 }]);
        assert!(find_hits(&lines, "").is_empty());
        let many: Vec<String> = vec!["aa".to_string(); MAX_HITS];
        assert_eq!(find_hits(&many, "a").len(), MAX_HITS, "capped");
        let unicode = vec!["caf\u{e9} CAF\u{c9}".to_string()];
        let h = find_hits(&unicode, "caf\u{e9}");
        let mixed = vec!["\u{c9}COLE \u{e9}cole ecole".to_string(), "plain ASCII Ecole".to_string()];
        let m = find_hits(&mixed, "\u{c9}cole");
        assert_eq!(m.len(), 2, "a non-ASCII needle matches either case, and never the ASCII spelling");
        assert_eq!(find_hits(&mixed, "ECOLE").len(), 2, "an ASCII needle on a non-ASCII line and on an ASCII line");
        assert_eq!(h.len(), 2);
        assert_eq!(&unicode[0][h[1].start..h[1].end], "CAF\u{c9}", "byte ranges land on char boundaries");
    }

    #[test]
    fn a_changed_fingerprint_reloads_and_keeps_the_scroll_clamped() {
        // The pure part of a reload: a shorter file pulls the scroll back; the find re-runs.
        let mut content = Content::Text { lines: Arc::new((0..100).map(|i| format!("line {i}")).collect()), spans: vec![Vec::new(); 100] };
        let mut scroll = ScrollAnimator::new();
        scroll.jump_to(80.0);
        let shorter = Content::Text { lines: Arc::new((0..20).map(|i| format!("line {i}")).collect()), spans: vec![Vec::new(); 20] };
        let before = std::mem::replace(&mut content, shorter);
        assert!(matches!(before, Content::Text { ref lines, .. } if lines.len() == 100));
        let max = match &content {
            Content::Text { lines, .. } => lines.len().saturating_sub(1) as f32,
            _ => 0.0,
        };
        scroll.clamp_max(max);
        assert_eq!(scroll.current, 19.0);
        let hits = match &content {
            Content::Text { lines, .. } => find_hits(lines, "line 1"),
            _ => Vec::new(),
        };
        assert_eq!(hits.len(), 11, "line 1 and line 10 to 19");
    }

    #[test]
    fn a_file_is_read_as_text_plain_over_the_limit_binary_or_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "one\n\ttwo\n").unwrap();
        let l = read_file(&text, None, None, 100, false).expect("loaded");
        assert_eq!(l.content, Content::Text { lines: Arc::new(vec!["one".into(), "    two".into()]), spans: vec![Vec::new(), Vec::new()] });
        assert!(l.plain.is_none() && l.fingerprint.is_some());
        assert!(read_file(&text, l.fingerprint, None, 100, false).is_none(), "an unchanged file is not read again");
        let big = read_file(&text, None, None, 4, false).expect("loaded");
        assert_eq!(big.plain, Some(Plain::TooBig), "over the limit: the text, plain, under a banner");
        assert!(matches!(big.content, Content::Text { ref lines, .. } if lines.len() == 2));
        let bin = dir.path().join("b.bin");
        std::fs::write(&bin, b"\x89PNG\0\0").unwrap();
        assert_eq!(read_file(&bin, None, None, 100, false).unwrap().content, Content::Binary);
        let huge = dir.path().join("huge.log");
        // Sparse: no disk or memory goes to the length.
        std::fs::File::create(&huge).unwrap().set_len(MAX_PLAIN_BYTES + 1).unwrap();
        assert_eq!(read_file(&huge, None, None, 100, false).unwrap().content, Content::TooLarge(MAX_PLAIN_BYTES + 1), "not read at all");
        assert_eq!(read_file(dir.path(), None, None, 100, false).unwrap().content, Content::Unreadable("is a directory".into()), "a directory is never read");
        let gone = read_file(&dir.path().join("gone"), l.fingerprint, None, 100, false).expect("a deleted file is a change");
        assert!(matches!(gone.content, Content::Unreadable(_)));
        assert_eq!(gone.fingerprint, None);
    }

    #[test]
    fn a_file_with_a_line_over_the_line_limit_loads_plain() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("min.js");
        let long = "x".repeat(MAX_LINE_BYTES + 1);
        std::fs::write(&path, format!("short\n{long}\n")).unwrap();
        let l = read_file(&path, None, None, 1 << 20, false).expect("loaded");
        assert_eq!(l.plain, Some(Plain::LongLine), "shown plain, under its own banner");
        assert!(matches!(l.content, Content::Text { ref lines, .. } if lines.len() == 2 && lines[1] == long), "the line is kept whole; only painting cuts it");
        std::fs::write(&path, format!("short\n{}\n", "x".repeat(MAX_LINE_BYTES))).unwrap();
        assert_eq!(read_file(&path, None, None, 1 << 20, false).unwrap().plain, None, "at the limit it is highlighted");
    }

    #[test]
    fn a_load_in_a_repository_carries_the_diff_against_head_and_none_when_clean() {
        let _lock = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| assert!(crate::explorer::fixture_git(&root).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success(), "git {args:?}");
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        let path = root.join("a.txt");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        assert_eq!(read_file(&path, None, None, 100, true).unwrap().diff, None, "a clean file has no diff");
        std::fs::write(&path, "one\nTWO\n").unwrap();
        let l = read_file(&path, None, None, 100, true).unwrap();
        let d = l.diff.expect("a changed tracked file");
        assert_eq!(d.marks, vec![None, Some(crate::diff::Mark::Modified)]);
        assert_eq!(d.deleted_before, vec![(2, vec!["three".to_string()])]);
        assert_eq!(read_file(&path, None, None, 100, false).unwrap().diff, None, "not asked for");
        assert_eq!(read_file(&path, None, None, 4, true).unwrap().diff, None, "plain for its size: no diff");
    }

    #[test]
    fn the_count_says_where_the_current_hit_is_and_when_the_search_was_capped() {
        assert_eq!(count_text(0, 0), "no matches");
        assert_eq!(count_text(2, 5), "3 of 5");
        assert_eq!(count_text(0, MAX_HITS), "1 of 10000+");
    }

    #[test]
    fn a_hit_is_stepped_through_with_wraparound() {
        assert_eq!(step(0, 3, 1), 1);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }

    #[test]
    fn only_a_result_newer_than_the_applied_one_lands() {
        assert!(should_apply(1, 0), "the first load");
        assert!(should_apply(5, 3), "newer than what is shown");
        assert!(!should_apply(3, 3), "already shown");
        assert!(!should_apply(2, 3), "an older load finishing late");
    }
}
