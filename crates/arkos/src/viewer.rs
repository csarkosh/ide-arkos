//! A read-only viewer tab (spec 6): the file loaded once off the UI thread, classified (text,
//! too big to highlight, binary), split into lines and highlighted; a find over the lines; a
//! scroll position in lines (in display rows while the inline diff shows deleted lines);
//! reloaded when the poll sees the file change. A file in a repository carries its diff
//! against HEAD (`diff`), recomputed on every load and on the ten-second timer.
//!
//! A markdown file that is clean UTF-8 with one kind of line ending, and that the user may write,
//! is also an editable `edit::Document`: edits mark the tab dirty and autosave a second later through `save_text`,
//! and a change on disk either reloads the file or, over unsaved edits, raises a conflict.

use crate::config_watch::fingerprint;
use crate::diff::{self, DiffMarks};
use crate::edit;
use crate::find::{FindLook, BAR_TOP};
use crate::highlight::{self, Highlighter};
use crate::line_edit::{self, Edit, LineEdit};
use crate::md_blocks;
use crate::md_fold;
use crate::md_format;
use crate::md_slash;
use crate::md_reveal;
use crate::md_table;
use crate::md_view;
use crate::media;
use crate::mermaid;
use crate::motion::{self, Fades};
use crate::palette::Rgb;
use crate::remote_image;
use crate::scroll::ScrollAnimator;
use crate::theme::Theme;
use crate::tree::{PaneId, Tab, TabId, ViewMode};
use crate::workspace::{self, DetailsOpen};
use crate::viewer;
use crate::viewer_element::{self as element, RawLayout};
use crate::Root;
use gpui::{
    div, prelude::*, px, App, Bounds, ClickEvent, ClipboardItem, Context, Div, Entity, EntityId, EntityInputHandler, EventEmitter, FocusHandle, KeyDownEvent, Keystroke, ListAlignment,
    ListOffset, ListState, MouseButton, MouseDownEvent, Pixels, Point, SharedString, UTF16Selection, Window,
};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Bytes a binary check looks at: a NUL in the first 8 KiB means not text.
pub const SNIFF: usize = 8 * 1024;
pub const MAX_HITS: usize = 10_000;
/// Files above this are not read at all: a notice says how big they are.
pub const MAX_PLAIN_BYTES: u64 = 64 << 20;
/// A file with a line longer than this (a minified bundle, a one-line JSON dump) is shown
/// plain: the grammars would chew on the whole line at once.
pub const MAX_LINE_BYTES: usize = 8 * 1024;
/// How long after the last edit the document is written.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);
/// How long a close or a quit waits for a save already on its way before writing itself.
pub const SAVE_WAIT: Duration = Duration::from_millis(500);
/// Why a tab with a conflict does not close or a quit stops, beside the banner that says it.
pub const CONFLICT_NOTICE: &str = "the file changed on disk; choose Reload or Keep mine first";

/// Numbers the temp files of saves, so two saves in flight never share one.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Whether a viewer of `path` holds an editable markdown document: the extension is `md`,
/// `markdown` or `mdx`, in any case.
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["md", "markdown", "mdx"].iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// Why a markdown file is shown read-only instead of as a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyWhy {
    /// The bytes are not strict UTF-8; saving the lossy text would corrupt them.
    NotUtf8,
    /// `\r\n` and a bare `\n` both occur; a save could keep only one.
    MixedEndings,
    /// The user may not write the file; an atomic save would replace it anyway.
    NotWritable,
}

/// What the viewer tells `Root`, which keeps the tab bar and the preview tab in step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerEvent {
    /// The document changed: the tab is dirty and is no longer a preview.
    Edited,
    Saved,
    SaveFailed,
    /// A link to a markdown file was Cmd+clicked: it opens in a viewer tab of its own.
    /// A markdown file to open, at the heading the anchor names when there is one.
    OpenPath(PathBuf, Option<String>),
    /// A local video's poster was Cmd+clicked: the file (canonical, checked by
    /// `media::checked_open_path`) opens in a video tab beside the document.
    OpenVideo(PathBuf),
    /// A link to a local HTML page was Cmd+clicked: it opens in a web tab beside the document.
    OpenPage(PathBuf),
    /// A `<details>` section was opened or closed: the file's section states now, for the
    /// workspace file.
    DetailsToggled(Vec<DetailsOpen>),
    /// The folded headings changed (`Viewer::folded`), for the workspace and the file's other tabs.
    FoldsChanged(Vec<workspace::Fold>),
    /// The diagrams shown as code or split changed, for the workspace and the file's other tabs.
    DiagramModesChanged(Vec<workspace::DiagramView>),
}

/// What a press on a media block in the rendered view does (`md_view::media_element`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosterPress {
    /// A plain press: the row's, which puts the caret in the block.
    Caret,
    /// Cmd+click follows the link around it, or opens remote media's web address.
    Link,
    /// Cmd+click on a local video: a video tab beside the document (spec 9).
    OpenVideo,
    /// Cmd+click on local audio: QuickTime Player plays it.
    QuickTime,
    /// Cmd+click on a local `webm`, which nothing here plays: Finder shows it.
    Reveal,
}

/// The press on a media block: `platform` is Cmd, `kind` what the path's name says (`None` for
/// a `webm`), `link` the link around it, `remote` whether the path is a web address.
pub fn poster_press(platform: bool, kind: Option<crate::media::Media>, link: Option<&str>, remote: bool) -> PosterPress {
    use crate::media::Media;
    match (platform, kind) {
        (false, _) => PosterPress::Caret,
        _ if link.is_some() || remote => PosterPress::Link,
        (true, Some(Media::Video)) => PosterPress::OpenVideo,
        (true, Some(Media::Audio)) => PosterPress::QuickTime,
        (true, None) => PosterPress::Reveal,
    }
}

/// What a load that found the file changed does to a viewer holding a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskChange {
    /// The file is what the viewer last wrote or read, or a save is in flight and the change is
    /// looked at again when it lands.
    Ignore,
    /// Someone else changed the file under unsaved edits: the user decides.
    Conflict,
    Reload,
}

/// What a load that found the file changed should do. `disk` is the file's fingerprint now,
/// `known` the one the viewer holds.
pub fn disk_change(disk: Option<(u128, u64)>, known: Option<(u128, u64)>, dirty: bool, saving: bool) -> DiskChange {
    if disk == known || saving {
        DiskChange::Ignore
    } else if dirty {
        DiskChange::Conflict
    } else {
        DiskChange::Reload
    }
}

/// Whether the autosave timer that carried `carried` may write now: no newer edit rescheduled
/// it, there is something to write, no conflict waits for the user, no save is in flight, and no
/// input method composition is half done.
pub fn autosave_due(carried: u64, save_seq: u64, dirty: bool, conflict: bool, saving: bool, composing: bool) -> bool {
    carried == save_seq && dirty && !conflict && !saving && !composing
}

/// The part of a viewer the conflict buttons change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictState {
    pub conflict: bool,
    pub dirty: bool,
    pub save_seq: u64,
}

/// What follows a conflict button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictAction {
    /// Keep mine: write the document over the file now.
    SaveNow,
    /// Reload: drop the edits and read the file again.
    Reload,
}

impl ConflictState {
    /// "Keep mine" keeps the edits and saves them; "Reload" drops them and bumps the sequence, so
    /// an autosave timer still pending finds nothing to write.
    pub fn resolve(self, keep_mine: bool) -> (ConflictState, ConflictAction) {
        if keep_mine {
            (ConflictState { conflict: false, ..self }, ConflictAction::SaveNow)
        } else {
            (ConflictState { conflict: false, dirty: false, save_seq: self.save_seq + 1 }, ConflictAction::Reload)
        }
    }
}

#[derive(Debug)]
pub enum SaveError {
    /// The file is not the one the viewer last read or wrote: another writer got there first, or
    /// it went away. Nothing was written.
    Changed,
    /// A close or a quit wrote newer text while this save waited; its last step was skipped so
    /// the older text never lands. Nothing of it reached the file.
    Superseded,
    Io(std::io::Error),
}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// Writes `text` to `path` and returns the file's fingerprint taken right after the write, so
/// the watcher's next look at the file sees nothing new. With `expect`, a file whose fingerprint
/// is not `expect` is left alone (`SaveError::Changed`); `None` writes regardless ("Keep mine").
///
/// A regular file is replaced atomically: a temp file `.{name}.arkos-{pid}-{n}.tmp` beside it
/// takes the text and the target's permissions, is synced, and is renamed over the target. A
/// directory that refuses the temp file refuses the save; the file is never half written. A
/// symlink is written in place through the link, since a rename would replace the link itself.
pub fn save_text(path: &Path, text: &str, expect: Option<(u128, u64)>) -> Result<(u128, u64), SaveError> {
    save_text_guarded(path, text, expect, || Some(()))
}

/// `save_text` with a gate before the step that changes the file (the rename, or for a symlink
/// the write in place): `gate` returns a guard held across that step, or `None` to skip it, which
/// leaves the file as it was and returns `SaveError::Superseded`. The guard is not held across the
/// temp file's write or its sync.
pub fn save_text_guarded<G>(path: &Path, text: &str, expect: Option<(u128, u64)>, gate: impl FnOnce() -> Option<G>) -> Result<(u128, u64), SaveError> {
    if expect.is_some() && fingerprint(path) != expect {
        return Err(SaveError::Changed);
    }
    // The rename would replace a file the user may not write, as long as its directory allows it.
    if std::fs::metadata(path).is_ok() && !writable(path) {
        return Err(SaveError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the file is read-only")));
    }
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if is_link {
        let Some(guard) = gate() else { return Err(SaveError::Superseded) };
        // Opening follows the link; without `create` a dangling link is an error, not a new file.
        let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
        f.write_all(text.as_bytes())?;
        drop(guard);
        f.sync_all()?;
    } else {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name = path.file_name().ok_or_else(|| std::io::Error::other("the path has no file name"))?;
        let tmp = parent.join(format!(".{}.arkos-{}-{}.tmp", name.to_string_lossy(), std::process::id(), TEMP_SEQ.fetch_add(1, Ordering::Relaxed)));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let written = (|| {
            file.write_all(text.as_bytes())?;
            if let Ok(meta) = std::fs::metadata(path) {
                file.set_permissions(meta.permissions())?;
            }
            file.sync_all()?;
            let Some(guard) = gate() else { return Ok(false) };
            std::fs::rename(&tmp, path)?;
            drop(guard);
            Ok(true)
        })();
        match written {
            Ok(true) => {}
            Ok(false) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(SaveError::Superseded);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(SaveError::Io(e));
            }
        }
    }
    fingerprint(path).ok_or_else(|| SaveError::Io(std::io::Error::other("the file is gone right after the write")))
}

/// Where the unsaved text of `source` goes when arkos drops it anyway: `<dir>/<file stem>-<unix
/// time>.md`, and from the second `attempt` on `<file stem>-<unix time>-<attempt>.md`, so two files
/// of one name dropped in the same second never share a recovery file.
pub fn recovery_path(dir: &Path, source: &Path, unix_secs: u64, attempt: u32) -> PathBuf {
    let stem = source.file_stem().map_or_else(|| "untitled".into(), |s| s.to_string_lossy());
    let name = if attempt == 0 { format!("{stem}-{unix_secs}.md") } else { format!("{stem}-{unix_secs}-{attempt}.md") };
    dir.join(name)
}

/// Writes `text`, the unsaved edits of `source`, to a new file in `dir` (created on demand) and
/// returns its path. An existing file is never replaced: the next free `recovery_path` is taken.
pub fn write_recovery(dir: &Path, source: &Path, text: &str, unix_secs: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for attempt in 0..1000 {
        let path = recovery_path(dir, source, unix_secs, attempt);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free recovery file name"))
}

/// Whether this process may write the file at `path` (through a symlink, to its target):
/// `access(2)` with `W_OK`, which weighs the owner, the group and the mode as a write would.
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call; access only reads it.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// A background save as a close or a quit sees it: they may not block on the UI thread for the
/// save to land, so the writer reports here as soon as the write is done, and a close that gave
/// up waiting stops the write's last step.
#[derive(Default)]
pub struct SaveFlight {
    state: Mutex<FlightState>,
    finished: Condvar,
}

#[derive(Default)]
struct FlightState {
    /// `Some` once the write finished: the fingerprint it left, or `None` when it failed.
    done: Option<Option<(u128, u64)>>,
    /// A close or a quit stopped waiting and writes newer text itself: the rename must not
    /// happen. Checked by the writer under the same lock it holds across the rename.
    abandoned: bool,
}

impl SaveFlight {
    /// The writer's gate before its rename: the lock, held across the rename, or `None` once the
    /// save was abandoned.
    fn gate(&self) -> Option<std::sync::MutexGuard<'_, FlightState>> {
        self.state.lock().ok().filter(|s| !s.abandoned)
    }

    fn finish(&self, fp: Option<(u128, u64)>) {
        if let Ok(mut state) = self.state.lock() {
            state.done = Some(fp);
        }
        self.finished.notify_all();
    }

    /// Waits up to `limit` for the write. When it is still running then, it is abandoned before
    /// this returns `None`, so its rename never lands over what the caller writes next.
    fn wait_or_abandon(&self, limit: Duration) -> Option<Option<(u128, u64)>> {
        let state = self.state.lock().ok()?;
        let (mut state, _) = self.finished.wait_timeout_while(state, limit, |s| s.done.is_none()).ok()?;
        if state.done.is_none() {
            state.abandoned = true;
        }
        state.done
    }
}

/// The document a markdown file becomes, or why it stays read-only: strict UTF-8 with one kind
/// of line ending. A carriage return without a line feed after it (an old Mac file, a stray one at
/// the end) counts as mixed: the document could not write it back.
fn markdown_document(bytes: &[u8]) -> Result<edit::Document, ReadOnlyWhy> {
    let text = std::str::from_utf8(bytes).map_err(|_| ReadOnlyWhy::NotUtf8)?;
    let crlf = text.matches("\r\n").count();
    let lone_cr = text.matches('\r').count() != crlf;
    if lone_cr || (crlf != 0 && crlf != text.matches('\n').count()) {
        return Err(ReadOnlyWhy::MixedEndings);
    }
    Ok(edit::Document::from_text(text))
}

/// Whether two documents write the same bytes: the same lines, line ending, byte order mark and
/// final line break.
fn same_text(a: &edit::Document, b: &edit::Document) -> bool {
    a.ending == b.ending && a.bom == b.bom && a.trailing_newline == b.trailing_newline && a.lines() == b.lines()
}

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

/// A NUL in the first `SNIFF` bytes: the viewer calls such a file binary.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(SNIFF)].contains(&0)
}

pub fn classify(bytes: &[u8], limit: usize) -> Classified {
    if looks_binary(bytes) {
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

/// A find hit as a selection: the anchor at its start, the head at its end, in characters. `None`
/// for a hit that does not fit the lines (found in lines an edit has since replaced).
pub fn hit_selection(lines: &[String], h: Hit) -> Option<edit::Caret> {
    let line = lines.get(h.line)?;
    if h.start > h.end || h.end > line.len() || !line.is_char_boundary(h.start) || !line.is_char_boundary(h.end) {
        return None;
    }
    let at = |byte: usize| edit::Pos { line: h.line, ch: line[..byte].chars().count() };
    Some(edit::Caret { anchor: at(h.start), head: at(h.end), goal_col: None })
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
    /// The editable document, built on the background thread for a markdown file that is strict
    /// UTF-8 with one kind of line ending and that the user may write (a byte order mark is set
    /// aside in `Document.bom`).
    pub doc: Option<edit::Document>,
    /// Why a markdown file is shown read-only; `None` for every other file.
    pub read_only: Option<ReadOnlyWhy>,
    /// The document's top-level blocks, split on the background thread with it; empty without a
    /// document.
    pub blocks: Vec<md_blocks::Block>,
    /// Whether a grammar other than Plain Text coloured the text (`Highlighter::syntax_for`);
    /// `None` for text the grammars would colour while they are still loading.
    pub coloured: Option<bool>,
}

pub struct Viewer {
    pub path: PathBuf,
    pub content: Content,
    /// `false` until the first load lands.
    pub loaded: bool,
    /// The view of this file is recorded (`telemetry::viewed`): at the first load whose format is
    /// known, which for code is the first one after the grammars loaded.
    pub viewed: bool,
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
    /// A markdown file's editable text; `content.lines` shares its lines.
    pub doc: Option<edit::Document>,
    pub caret: edit::Caret,
    /// The document's top-level blocks, spliced on every edit.
    pub blocks: Vec<md_blocks::Block>,
    pub mode: ViewMode,
    /// The rendered view's rows, one per block, spliced with `blocks`: gpui keeps each row's
    /// measured height and the scroll position in it.
    pub list_state: ListState,
    /// Each block's inline runs (and a code block's colours), filled when the row is first drawn
    /// and spliced with `blocks`, so an edit only drops the blocks it changed.
    pub block_cache: Vec<Option<md_view::Rendered>>,
    /// Edits not yet on disk: the tab's label ends in " •".
    pub dirty: bool,
    /// The file changed on disk under unsaved edits; nothing is written until the user picks.
    pub conflict: bool,
    /// A background save is on its way.
    pub saving: bool,
    /// An autosave came due while a save was in flight: save again when it lands.
    pub save_again: bool,
    /// The last save's failure, for the banner.
    pub save_error: Option<String>,
    /// Numbers every edit (and a dropped edit set), so a save knows whether what it wrote is still
    /// the latest and an autosave timer whether a newer edit took over.
    pub save_seq: u64,
    /// The input method's uncommitted text, while a composition is in progress.
    pub marked: Option<Range<edit::Pos>>,
    pub read_only: Option<ReadOnlyWhy>,
    /// A watcher pass was ignored because a save was in flight: look at the file again once it
    /// lands.
    pub recheck_after_save: bool,
    /// The save in flight with the sequence it wrote, for a close or a quit to wait on.
    flight: Option<(u64, Arc<SaveFlight>)>,
    /// The edit (`save_seq`) whose text was last written to the recovery folder, so a second
    /// flush on the way out does not write the same text again.
    recovered: Option<u64>,
    /// How far the raw text is scrolled to the right, in pixels: the raw view does not wrap, so it
    /// follows the caret sideways.
    pub scroll_x: f32,
    /// Where the element last laid the text out, written in prepaint: the input handler and the
    /// mouse map between positions and pixels with it. `None` until a document is painted.
    pub last_bounds: Option<RawLayout>,
    /// Where the rendered view's raw blocks were laid out this frame, top to bottom: the input
    /// handler and the mouse map between positions and pixels with them. Emptied when the view is
    /// built, filled as the raw blocks are laid out.
    pub raw_blocks: Vec<md_view::BlockLayout>,
    /// Where the rendered view's revealed texts were laid out this frame (a paragraph's, a
    /// heading's, a quote's, each revealed item's), emptied and filled as `raw_blocks` is.
    pub reveal_blocks: Vec<md_view::RevealLayout>,
    /// The x Up and Down aim for in a revealed text, with the caret position it holds for: a move
    /// that lands elsewhere makes the next one start from the caret's own x.
    reveal_goal: Option<(edit::Pos, f32)>,
    /// The slash menu while it is open (`md_slash`).
    pub slash: Option<Slash>,
    /// Where the open slash menu's `/` was laid out, in window pixels, with its line height:
    /// recorded as the revealed text holding it is laid out (`note_slash_layout`), so the menu,
    /// built before that layout in a frame, hangs from the last one.
    pub slash_at: Option<(Point<Pixels>, Pixels)>,
    /// A code block's language list while it is open (`md_code`), and where its pill was laid out.
    pub lang: Option<LangPicker>,
    pub lang_at: Option<Point<Pixels>>,
    /// The code block whose Copy was clicked, and when: its button reads "Copied" for a moment.
    pub copied: Option<(usize, Instant)>,
    /// A heading to scroll to once the document has loaded (a link's `#anchor` into this file).
    pub pending_anchor: Option<String>,
    /// The document's footnotes (`md_blocks::footnotes`), made again with the headings.
    pub notes: Rc<md_blocks::Notes>,
    /// Presentation mode: read-only, no caret and no reveal, a single click follows a link.
    pub presenting: bool,
    /// The diagrams shown as code or split, by their index among the document's diagrams.
    pub diagram_modes: std::collections::BTreeMap<usize, workspace::DiagramMode>,
    /// Each diagram's zoom and pan, by its index, for this view only (`md_diagram`).
    pub diagram_zoom: HashMap<usize, (f32, (f32, f32))>,
    /// A pan under way: the diagram, where the press was and the pan it started from.
    pub diagram_drag: Option<(usize, Point<Pixels>, (f32, f32))>,
    /// When the document last changed, so a diagram being typed is drawn once the typing pauses.
    pub edited_at: Instant,
    /// The last picture each Split diagram being edited had, by its block (`md_view::split_preview`).
    pub split_last: HashMap<usize, (PathBuf, u32, u32)>,
    /// The config's `viewer.image_destination`, kept here by `Root` (`md_links::image_dir`).
    pub image_destination: String,
    /// A table cell is being edited (its markdown shown with the caret), not only selected.
    pub table_edit: bool,
    /// A table's row, column or whole grid picked by its edge, with the table's block.
    pub table_pick: Option<(usize, Pick)>,
    /// The user has put the caret somewhere since the file was opened or reloaded (a press in the
    /// text, a handled key, typing): until then the rendered view shows every block rendered, so a
    /// freshly opened file has no raw block at the caret's default place.
    caret_placed: bool,
    /// A click on the rendered view's tail put the caret in a new paragraph that is not in the
    /// document yet: its blank lines are added with the first edit (`take_pending_append`), so a
    /// click alone leaves the file as it is. The caret sits at the document's end meanwhile.
    pub pending_append: bool,
    /// The composition `marked` belongs to; `Some` exactly while `marked` is.
    compose: Option<Compose>,
    /// A left-button drag that started in the text is selecting.
    selecting: bool,
    /// The table block the caret is in, in the rendered view: its start line and its text
    /// (`table_text`) at the caret's entry or at the last undo or redo, whichever came later.
    /// Leaving it realigns its pipes when the text differs (`note_caret_moved`, `next_entry`).
    /// Edits, including `edit_elsewhere`, leave it as it is.
    table_entry: Option<(usize, String)>,
    /// The web images the rendered view has shown, by url: fetching, in the cache, or failed
    /// (`md_view::fill` moves them along; a failed one is fetched again only when the file is
    /// opened again, in a new viewer).
    pub remote: HashMap<String, remote_image::Remote>,
    /// The posters of the local videos the rendered view has shown, by file: being made, in the
    /// cache, or failed (`md_view::fill` moves them along, reading each file's mtime there and not
    /// at every frame).
    pub posters: HashMap<PathBuf, media::Poster>,
    /// The sizes of the local audio files the rendered view has shown, read once in
    /// `md_view::fill`; `None` when the file cannot be read.
    pub sizes: HashMap<PathBuf, Option<u64>>,
    /// The diagrams the rendered view has shown, by cache key: rendering, in the cache with their
    /// size, failed, or not to be rendered without Chrome (`md_view::fill` moves them along; a
    /// failed one is rendered again only in a new viewer, once its file is opened again).
    pub diagrams: HashMap<String, mermaid::Diagram>,
    /// Whether each `<details>` section the user opened or closed is open, by its summary and its
    /// index among the file's sections, so an edit elsewhere in the file keeps the state. A
    /// section with no state here is shown as its `open` attribute says. Taken from the workspace
    /// when the viewer is made and saved there on every toggle (`ViewerEvent::DetailsToggled`).
    pub open_details: BTreeMap<(String, usize), bool>,
    /// The headings folded in the rendered view, by text and place among namesakes; taken from
    /// the workspace when the viewer is made and saved there on every change.
    pub folded: std::collections::BTreeSet<md_fold::FoldKey>,
    /// The document's headings and, per block, whether a fold hides it: made again after every
    /// edit, load and fold.
    pub heads: Vec<md_fold::Head>,
    pub hidden: Vec<bool>,
}

/// What a table's edge selectors pick: a row (the grid's, 0 the header), a column, or all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Row(usize),
    Column(usize),
    All,
}

/// The table menu's actions on the selected cell's row and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAction {
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
    DeleteTable,
    Align(md_table::Align),
}

/// A code block's language list: the block, the text typed in its search field, and the row
/// selected among the languages that text matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangPicker {
    pub block: usize,
    pub query: String,
    pub selected: usize,
}

/// The slash menu's state: where the `/` is (its line and column), the text typed after it, the
/// selected row, and the submenu open beside it with its own selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slash {
    pub line: usize,
    pub col: usize,
    pub query: String,
    pub selected: usize,
    pub sub: Option<usize>,
    pub sub_selected: usize,
}

impl Slash {
    /// The rows for the text typed (`md_slash::filter`).
    pub fn rows(&self) -> Vec<md_slash::Entry> {
        md_slash::filter(&self.query)
    }
}

/// Whether a `/` typed at column `ch` of `line` opens the slash menu: where a word could start
/// (the line's start, or after a space), in a paragraph, heading, list or quote (a `/` on a line of
/// its own is a paragraph), never in a code block, a table or HTML. `blocks` are the document's
/// after the `/` went in.
pub fn slash_opens(lines: &[String], blocks: &[md_blocks::Block], line: usize, ch: usize) -> bool {
    use md_blocks::BlockKind as K;
    let Some(l) = lines.get(line) else { return false };
    if l.chars().take(ch).last().is_some_and(|c| !c.is_whitespace()) {
        return false;
    }
    blocks.get(md_blocks::block_at(blocks, line)).is_none_or(|b| matches!(b.kind, K::Paragraph | K::Heading(_) | K::List | K::Quote | K::Blank))
}

/// An input method composition in progress: the caret before it began, and whether its marked
/// text is the undo step on top (taken back when the composition changes or is committed).
#[derive(Debug, Clone, Copy)]
struct Compose {
    before: edit::Caret,
    step: bool,
}

impl EventEmitter<ViewerEvent> for Viewer {}

impl Viewer {
    pub fn new(path: PathBuf, font_size: f32, line_height: f32, cx: &mut Context<Self>) -> Self {
        Viewer {
            path,
            content: Content::Text { lines: Arc::new(Vec::new()), spans: Vec::new() },
            loaded: false,
            viewed: false,
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
            doc: None,
            caret: edit::Caret::default(),
            blocks: Vec::new(),
            mode: ViewMode::default(),
            list_state: ListState::new(0, ListAlignment::Top, px(600.0)),
            block_cache: Vec::new(),
            dirty: false,
            conflict: false,
            saving: false,
            save_again: false,
            save_error: None,
            save_seq: 0,
            marked: None,
            read_only: None,
            recheck_after_save: false,
            flight: None,
            recovered: None,
            scroll_x: 0.0,
            last_bounds: None,
            raw_blocks: Vec::new(),
            reveal_blocks: Vec::new(),
            reveal_goal: None,
            slash: None,
            slash_at: None,
            lang: None,
            lang_at: None,
            copied: None,
            pending_anchor: None,
            notes: Rc::new(md_blocks::Notes::default()),
            presenting: false,
            diagram_modes: Default::default(),
            diagram_zoom: HashMap::new(),
            diagram_drag: None,
            edited_at: Instant::now(),
            split_last: HashMap::new(),
            image_destination: String::new(),
            table_edit: false,
            table_pick: None,
            caret_placed: false,
            pending_append: false,
            compose: None,
            selecting: false,
            table_entry: None,
            remote: HashMap::new(),
            posters: HashMap::new(),
            sizes: HashMap::new(),
            diagrams: HashMap::new(),
            open_details: BTreeMap::new(),
            folded: Default::default(),
            heads: Vec::new(),
            hidden: Vec::new(),
        }
    }

    /// Starts the poster job that has the turn in the process-wide queue (`media::queue`, one at
    /// a time), on the background executor (qlmanage blocks its thread). The job frees its turn
    /// itself, whether or not this viewer is still open; then this viewer starts the next one and
    /// draws again, and every view showing the video finds the poster in the cache
    /// (`md_view::fill`).
    pub fn start_thumbnails(cx: &mut Context<Self>) {
        let ready = media::queue().start_ready();
        for file in ready {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(e) = media::make_poster(&media::poster_dir(), &file) {
                            eprintln!("arkos: cannot make a poster for {}: {e}", file.display());
                        }
                        media::queue().done(file.as_path());
                    })
                    .await;
                let _ = this.update(cx, |_, cx| {
                    Viewer::start_thumbnails(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Starts the diagram or equation render that has the turn in the process-wide queue
    /// (`mermaid::queue`, one at a time) in the window's hidden web view (`render_view`): its page
    /// written and loaded, then the render moved along every few frames' worth of time on this
    /// thread until it has its picture or its error (`mermaid::finish`, which frees the turn,
    /// whether or not this viewer is still open). Then this viewer starts the next one and draws
    /// again, and every view showing the picture finds it in the cache, or its error
    /// (`md_view::fill`).
    pub fn start_renders(cx: &mut Context<Self>) {
        let ready = mermaid::queue().start_ready();
        for job in ready {
            let dir = mermaid::cache_dir();
            let started = mermaid::prepare(&dir, &job).and_then(|page| crate::render_view::Render::start(&page, &dir).map_err(|e| (e, Some(page))).map_err(|(e, page)| {
                if let Some(page) = page {
                    let _ = std::fs::remove_file(page);
                }
                e
            }));
            let mut render = match started {
                Ok(r) => r,
                Err(e) => {
                    mermaid::finish(&dir, &job.key, Path::new(""), Err(e));
                    cx.notify();
                    continue;
                }
            };
            cx.spawn(async move |this, cx| {
                let result = loop {
                    cx.background_executor().timer(Duration::from_millis(40)).await;
                    if let Some(result) = render.poll() {
                        break result;
                    }
                };
                mermaid::finish(&dir, &job.key, render.page(), result);
                let _ = this.update(cx, |_, cx| {
                    Viewer::start_renders(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Starts the web image fetches that have a slot in the process-wide queue
    /// (`remote_image::queue`), each on the background executor (curl blocks its thread, at most
    /// `remote_image::MAX_IN_FLIGHT` of them). The fetch frees its slot itself, whether or not
    /// this viewer is still open; then this viewer starts the next ones and draws again, and the
    /// window's frame lets every view showing the image find it in the cache (`md_view::fill`).
    pub fn start_fetches(cx: &mut Context<Self>) {
        let ready = remote_image::queue().start_ready();
        for url in ready {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(e) = remote_image::fetch(&remote_image::cache_dir(), &url) {
                            eprintln!("arkos: cannot fetch the image {url}: {e}");
                        }
                        remote_image::queue().done(&url);
                    })
                    .await;
                let _ = this.update(cx, |_, cx| {
                    Viewer::start_fetches(cx);
                    cx.notify();
                });
            })
            .detach();
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
        if self.inline_diff && self.doc.is_some() {
            crate::telemetry::md(crate::telemetry::MdFeature::InlineDiff);
        }
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
        crate::telemetry::viewed(&mut self.viewed, &loaded.content, loaded.plain.is_some(), loaded.coloured, &self.path, self.mode);
        self.content = loaded.content;
        self.fingerprint = loaded.fingerprint;
        self.plain = loaded.plain;
        self.diff = loaded.diff;
        self.read_only = loaded.read_only;
        // The rendered view keeps its place by line: the line the top row starts on.
        let old_top_line = self.blocks.get(self.list_state.logical_scroll_top().item_ix).map(|b| b.lines.start);
        match (self.doc.take(), loaded.doc) {
            // The same text again: the old document stays, and with it the undo history. The
            // lines are compared one by one, never joined.
            (Some(old), Some(new)) if same_text(&old, &new) => {
                self.content = Content::Text { lines: old.shared_lines(), spans: Vec::new() };
                self.doc = Some(old);
            }
            (_, new) => self.doc = new,
        }
        match &self.doc {
            Some(doc) => {
                self.caret = edit::Caret { head: doc.clamp(self.caret.head), anchor: doc.clamp(self.caret.anchor), goal_col: None };
                // Split with the load, off this thread; the same lines when the old document stayed.
                self.blocks = loaded.blocks;
            }
            None => {
                self.caret = edit::Caret::default();
                self.blocks.clear();
            }
        }
        reset_rows(&self.blocks, &mut self.block_cache, &self.list_state);
        self.refresh_structure();
        if let Some(line) = old_top_line.filter(|_| !self.blocks.is_empty()) {
            self.list_state.scroll_to(ListOffset { item_ix: md_blocks::block_at(&self.blocks, line), offset_in_item: px(0.0) });
        }
        // A link's `#anchor` into this file, waiting for the load.
        if let Some(anchor) = self.pending_anchor.take() {
            self.go_to_anchor(&anchor, cx);
        }
        // What is shown is what is on disk now.
        self.dirty = false;
        self.conflict = false;
        self.save_error = None;
        self.marked = None;
        self.compose = None;
        self.selecting = false;
        self.caret_placed = false;
        self.pending_append = false;
        // A table changed on disk is not the user's edit: the caret enters anew.
        self.table_entry = None;
        self.loaded = true;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            // The old hits point into the old lines.
            f.hits.clear();
        }
        self.search(false, cx);
        cx.notify();
    }

    /// Runs an edit on the document with the caret and the time, and takes the caret it returns.
    /// Nothing happens without a document. An input method composition in progress is finished
    /// first, keeping its text, so the edit never lands between its marked step and its commit.
    pub fn edit(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        self.finish_composition(cx);
        self.take_pending_append(cx);
        self.edit_raw(f, cx);
    }

    /// `edit` for a change away from the caret, such as a task box clicked in the rendered view:
    /// the view stays where it is instead of scrolling to the caret.
    pub fn edit_elsewhere(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>) {
        self.finish_composition(cx);
        self.run_edit(f, cx);
    }

    /// `edit` without the composition guard, for the input handler, which keeps the composition
    /// itself.
    fn edit_raw(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>) {
        self.run_edit(f, cx);
        // After `changed`: the row count the scroll is clamped to includes any new line.
        self.reveal_caret();
    }

    /// Runs the edit and takes in its result, without scrolling.
    fn run_edit(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>) {
        // Presentation mode reads (`set_presenting`): whatever asked, nothing changes.
        if self.presenting {
            return;
        }
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        self.caret = f(doc, self.caret, Instant::now());
        self.changed(old_lines, cx);
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.finish_composition(cx);
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        if let Some(caret) = doc.undo() {
            self.caret = caret;
            self.changed(old_lines, cx);
            // A caret put back inside a hidden marker goes where typing would land.
            self.snap_for_edit();
            self.reveal_caret();
            self.note_caret_moved(true, cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.finish_composition(cx);
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        if let Some(caret) = doc.redo() {
            self.caret = caret;
            self.changed(old_lines, cx);
            // A caret put back inside a hidden marker goes where typing would land.
            self.snap_for_edit();
            self.reveal_caret();
            self.note_caret_moved(true, cx);
        }
    }

    /// After an edit, undo or redo: the shown lines become the document's, the blocks are
    /// spliced, the diff waits for the next save, the tab is dirty and the autosave is scheduled.
    fn changed(&mut self, old_lines: Arc<Vec<String>>, cx: &mut Context<Self>) {
        let Some(doc) = &self.doc else { return };
        let new_lines = doc.shared_lines();
        // `old_lines` holds the buffer, so any edit copied it: the same buffer means no edit (an
        // empty insert, a delete at the document's edge), only perhaps a caret move.
        if Arc::ptr_eq(&old_lines, &new_lines) {
            cx.notify();
            return;
        }
        splice_rows(&mut self.blocks, &mut self.block_cache, &self.list_state, &old_lines, &new_lines);
        self.edited_at = Instant::now();
        self.content = Content::Text { lines: new_lines, spans: Vec::new() };
        self.refresh_structure();
        // The marks point at lines that may have moved; the next save computes them again.
        self.diff = None;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            // The old hits point into the old lines.
            f.hits.clear();
        }
        self.search(false, cx);
        self.refresh_slash();
        self.dirty = true;
        self.save_seq += 1;
        self.schedule_autosave(cx);
        cx.emit(ViewerEvent::Edited);
        cx.notify();
    }

    /// Saves `AUTOSAVE_DELAY` after this edit unless a newer edit, a reload or a conflict came
    /// first. A save still in flight then is asked to run again when it lands.
    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let seq = self.save_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            let _ = this.update(cx, |v, cx| {
                if seq == v.save_seq && v.saving {
                    v.save_again = true;
                } else if autosave_due(seq, v.save_seq, v.dirty, v.conflict, v.saving, v.marked.is_some()) {
                    v.save(cx);
                }
            });
        })
        .detach();
    }

    pub fn set_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // Only the rendered view presents.
        self.presenting = false;
        // The paragraph a click on the rendered view's tail promised belongs to that view: the
        // raw view's first keystroke must not add blank lines before it.
        self.pending_append = false;
        if let Some(doc) = &mut self.doc {
            doc.break_coalescing();
        }
        cx.notify();
    }

    /// Writes the document on the background executor, refusing when the file changed since it
    /// was read or last written (that raises the conflict instead). A save already in flight
    /// runs this again when it lands.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = &self.doc else { return };
        if self.saving {
            self.save_again = true;
            return;
        }
        if self.conflict {
            return;
        }
        self.saving = true;
        let (text, expect, seq, applied, path) = (doc.to_text(), self.fingerprint, self.save_seq, self.applied, self.path.clone());
        let flight = Arc::new(SaveFlight::default());
        self.flight = Some((seq, flight.clone()));
        cx.spawn(async move |this, cx| {
            let report = flight.clone();
            let (result, text) = cx
                .background_executor()
                .spawn(async move {
                    let result = save_text_guarded(&path, &text, expect, || report.gate());
                    report.finish(result.as_ref().ok().copied());
                    (result, text)
                })
                .await;
            let _ = this.update(cx, |v, cx| v.save_landed(result, seq, applied, text, &flight, cx));
        })
        .detach();
    }

    fn save_landed(&mut self, result: Result<(u128, u64), SaveError>, seq: u64, applied: u64, text: String, flight: &Arc<SaveFlight>, cx: &mut Context<Self>) {
        // A close that waited for this write took it over and already settled the state.
        if !self.flight.as_ref().is_some_and(|(_, f)| Arc::ptr_eq(f, flight)) {
            return;
        }
        self.flight = None;
        self.saving = false;
        match result {
            Ok(fp) => {
                self.fingerprint = Some(fp);
                // An edit made while the write ran is still to be written.
                self.dirty = self.save_seq != seq;
                self.save_error = None;
                self.rediff_saved(text, seq, applied, cx);
                cx.emit(ViewerEvent::Saved);
            }
            Err(SaveError::Changed) => {
                self.conflict = true;
                // The conflict is the news; no re-check is needed on top of it.
                self.recheck_after_save = false;
            }
            Err(SaveError::Io(e)) => {
                self.save_error = Some(format!("cannot save {}: {e}", self.path.display()));
                crate::telemetry::error(crate::telemetry::Failure::SaveFailed);
                cx.emit(ViewerEvent::SaveFailed);
            }
            // Only a close or a quit abandons a save, and it took the flight over first.
            Err(SaveError::Superseded) => {}
        }
        if std::mem::take(&mut self.save_again) && self.dirty && !self.conflict {
            // A composition in progress is not written half done: the timer comes back later.
            if self.marked.is_none() {
                self.save(cx);
            } else {
                self.schedule_autosave(cx);
            }
        }
        cx.notify();
    }

    /// The diff against HEAD for the text a save wrote, as `Root::rediff_viewers` computes it. It
    /// lands only while that text is still what the viewer shows.
    fn rediff_saved(&mut self, text: String, seq: u64, applied: u64, cx: &mut Context<Self>) {
        let path = self.path.clone();
        cx.spawn(async move |this, cx| {
            let diff = cx.background_executor().spawn(async move { diff_of(&path, &split_lines(&text)) }).await;
            let _ = this.update(cx, |v, cx| {
                if v.save_seq == seq && v.applied == applied {
                    v.set_diff(diff, cx);
                }
            });
        })
        .detach();
    }

    /// Writes unsaved edits now, on the calling thread, for a tab that closes or a window that
    /// quits. A save in flight is waited for, up to `SAVE_WAIT`; one still running then is written
    /// over without the fingerprint check (its own write may be what changed the file). `Err`
    /// says why the edits are not on disk: a conflict or a write error.
    pub fn save_now(&mut self) -> Result<(), String> {
        let Some(doc) = &self.doc else { return Ok(()) };
        if !self.dirty && !self.saving {
            return Ok(());
        }
        if self.conflict {
            return Err(CONFLICT_NOTICE.to_string());
        }
        let mut expect = self.fingerprint;
        if let Some((seq, flight)) = self.flight.take() {
            self.saving = false;
            match flight.wait_or_abandon(SAVE_WAIT) {
                Some(Some(fp)) => {
                    expect = Some(fp);
                    self.fingerprint = Some(fp);
                    if seq == self.save_seq {
                        self.dirty = false;
                    }
                }
                // It failed: the file is as it was, and so is the expectation.
                Some(None) => {}
                // Abandoned: it will not rename over this write, and it may have been the one
                // that changed the file, so no check.
                None => expect = None,
            }
        }
        if !self.dirty {
            return Ok(());
        }
        match save_text(&self.path, &doc.to_text(), expect) {
            Ok(fp) => {
                self.fingerprint = Some(fp);
                self.dirty = false;
                self.save_error = None;
                Ok(())
            }
            Err(SaveError::Changed) => {
                self.conflict = true;
                Err(CONFLICT_NOTICE.to_string())
            }
            Err(SaveError::Io(e)) => {
                let message = format!("cannot save {}: {e}", self.path.display());
                self.save_error = Some(message.clone());
                Err(message)
            }
            // `save_text` always passes its gate; this arm only keeps the match total.
            Err(SaveError::Superseded) => Err(format!("cannot save {}: superseded", self.path.display())),
        }
    }

    /// Writes edits that are not on disk to the recovery folder `dir`, for a quit or a relaunch
    /// that goes ahead without them: `None` when there is nothing to write (no document, nothing
    /// unsaved, or this text already written there).
    pub fn recover(&mut self, dir: &Path, unix_secs: u64) -> Option<std::io::Result<PathBuf>> {
        let doc = self.doc.as_ref().filter(|_| self.dirty && self.recovered != Some(self.save_seq))?;
        let written = write_recovery(dir, &self.path, &doc.to_text(), unix_secs);
        if written.is_ok() {
            self.recovered = Some(self.save_seq);
        }
        Some(written)
    }

    /// The conflict's two buttons: keep mine writes the document over the file at once, adopting
    /// the file's fingerprint so the save's check passes; reload drops the edits.
    pub fn resolve_conflict(&mut self, keep_mine: bool, cx: &mut Context<Self>) {
        let (next, action) = ConflictState { conflict: self.conflict, dirty: self.dirty, save_seq: self.save_seq }.resolve(keep_mine);
        self.conflict = next.conflict;
        self.dirty = next.dirty;
        self.save_seq = next.save_seq;
        match action {
            ConflictAction::SaveNow => {
                self.fingerprint = fingerprint(&self.path);
                self.save(cx);
            }
            ConflictAction::Reload => self.reload_document(cx),
        }
        cx.notify();
    }

    /// Reads the file again and shows it, whatever its fingerprint. A markdown file needs neither
    /// the highlighter nor the size limit. An edit made before it lands keeps the conflict up
    /// instead.
    fn reload_document(&mut self, cx: &mut Context<Self>) {
        self.loads += 1;
        let (seq, path) = (self.loads, self.path.clone());
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { read_file(&path, None, None, usize::MAX, true) }).await;
            let Some(loaded) = loaded else { return };
            let _ = this.update(cx, |v, cx| {
                if !should_apply(seq, v.applied) {
                    return;
                }
                if v.dirty {
                    v.conflict = true;
                    cx.notify();
                    return;
                }
                v.applied = seq;
                v.apply(loaded, cx);
            });
        })
        .detach();
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
        self.select_current_hit(cx);
        self.find = None;
        // A search still running lands in no later bar.
        self.find_seq += 1;
        // The current hit's block was raw while the bar was open; the view goes back to the caret.
        if self.rendered() {
            self.reveal_in_list();
        }
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
        self.select_current_hit(cx);
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

    /// A document's selection becomes the current find hit (the anchor at its start, the head at
    /// its end), so typing after the bar closes replaces it. The caret counts as placed.
    fn select_current_hit(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.find.as_ref().and_then(|f| f.hits.get(f.current)).copied() else { return };
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(caret) = hit_selection(doc.lines(), h) else { return };
        doc.break_coalescing();
        self.caret = caret;
        self.caret_placed = true;
        self.pending_append = false;
    }

    /// Scrolls so the current hit's line is in view, a third of the way down.
    fn show_current_hit(&mut self) {
        let Some(f) = &self.find else { return };
        let Some(h) = f.hits.get(f.current) else { return };
        if self.rendered() {
            let ix = md_blocks::block_at(&self.blocks, h.line);
            // A hit in a folded section shows it.
            for key in md_fold::hiding(&self.heads, &self.folded, ix, self.blocks.len()) {
                self.folded.remove(&key);
            }
            self.hidden = md_fold::hidden(&self.heads, &self.folded, self.blocks.len());
            self.list_state.scroll_to_reveal_item(ix);
            return;
        }
        let target = (self.row_of_line(h.line) as f32 - (self.rows_visible as f32 / 3.0).floor()).max(0.0);
        self.scroll.target = target.min(self.max_scroll());
    }

    /// An editing key on a document viewer: `true` when it was handled, and the caller stops its
    /// propagation (gpui would hand Enter and Tab to the input method too). Edits run
    /// through `edit`, so the tab turns dirty and the autosave is scheduled; moves only end the
    /// typing step. A key this does not know returns `false` for the keymap and the input method.
    pub fn key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.doc.is_none() {
            return false;
        }
        // Presenting, the keys scroll and nothing else (a caret is never placed).
        if self.presenting {
            return self.key_before_placement_reading(ks, cx);
        }
        if self.slash.is_some() && self.slash_key(ks, cx) {
            return true;
        }
        if self.lang.is_some() && self.lang_key(ks, cx) {
            return true;
        }
        if self.table_key(ks, cx) {
            return true;
        }
        if self.rendered() && !self.caret_placed {
            return self.key_before_placement(ks, cx);
        }
        let action = key_action(ks);
        let handled = self.key_inner(ks, cx);
        // A caret move or an edit puts the caret in play: the rendered view shows its block raw.
        if handled && action.is_some_and(places_caret) {
            self.caret_placed = true;
        }
        handled
    }

    /// A key in the rendered view before the caret has been placed, which would otherwise act on
    /// a caret the user cannot see (`pre_placement_action`): Space and the page keys scroll as in
    /// the read-only viewer, an editing key first puts the caret at the start of the first block
    /// on screen. Text typed comes through the input handler, which places the caret the same way.
    fn key_before_placement(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &ks.modifiers;
        if ks.key == "space" && !(m.platform || m.control || m.alt) {
            self.list_state.scroll_by(self.list_state.viewport_bounds().size.height);
            cx.notify();
            return true;
        }
        let Some(action) = key_action(ks) else { return false };
        match action {
            // Saving is not about the caret.
            KeyAction::Save => {
                self.save_command(cx);
                return true;
            }
            // The find bar closes, as Escape always does; that puts the caret on its hit.
            KeyAction::Escape if self.find.is_some() => {
                self.close_find(cx);
                return true;
            }
            _ => {}
        }
        match pre_placement_action(action) {
            PrePlacement::Ignore => {}
            PrePlacement::Scroll => {
                self.scroll_list(action);
                cx.notify();
            }
            PrePlacement::Place => {
                self.place_at_first_visible();
                self.key_inner(ks, cx);
            }
        }
        true
    }

    /// A key in presentation mode: Space and the scroll keys scroll, Escape closes the find bar,
    /// and every other key the keymap left to the document is taken and does nothing.
    fn key_before_placement_reading(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &ks.modifiers;
        if ks.key == "space" && !(m.platform || m.control || m.alt) {
            self.list_state.scroll_by(self.list_state.viewport_bounds().size.height);
            cx.notify();
            return true;
        }
        let Some(action) = key_action(ks) else { return false };
        match action {
            KeyAction::Escape if self.find.is_some() => self.close_find(cx),
            action if pre_placement_action(action) == PrePlacement::Scroll => {
                self.scroll_list(action);
                cx.notify();
            }
            _ => {}
        }
        true
    }

    /// The scroll keys of the rendered view: the page keys by the list's height, Home and End to
    /// its ends, Up and Down by a line.
    fn scroll_list(&mut self, action: KeyAction) {
        let page = self.list_state.viewport_bounds().size.height;
        let line = px(self.font_size * self.line_height);
        match action {
            KeyAction::Move(edit::Move::PageDown(_), _) => self.list_state.scroll_by(page),
            KeyAction::Move(edit::Move::PageUp(_), _) => self.list_state.scroll_by(-page),
            KeyAction::Move(edit::Move::Down, _) => self.list_state.scroll_by(line),
            KeyAction::Move(edit::Move::Up, _) => self.list_state.scroll_by(-line),
            KeyAction::ScrollHome => self.list_state.scroll_to(ListOffset::default()),
            KeyAction::ScrollEnd => self.list_state.scroll_to(ListOffset { item_ix: self.list_state.item_count(), offset_in_item: px(0.0) }),
            _ => {}
        }
    }

    /// Puts the caret at the start of the first block on screen in the rendered view, where an
    /// edit made before the user placed it lands.
    fn place_at_first_visible(&mut self) {
        let Some(doc) = self.doc.as_mut() else { return };
        let ix = self.list_state.logical_scroll_top().item_ix.min(self.blocks.len().saturating_sub(1));
        let line = self.blocks.get(ix).map_or(0, |b| b.lines.start);
        doc.break_coalescing();
        self.caret = edit::Caret::at(doc.clamp(edit::Pos { line, ch: 0 }));
        self.caret_placed = true;
    }

    /// The input handler's first step: in the rendered view before the caret has been placed, the
    /// caret goes to the first block on screen, and a paragraph a click on the tail promised is
    /// written. `true` when the caret moved, and a range the input method gave against the old
    /// field no longer applies.
    fn place_for_input(&mut self, cx: &mut Context<Self>) -> bool {
        if self.rendered() && !self.caret_placed {
            self.place_at_first_visible();
            return true;
        }
        self.caret_placed = true;
        if self.pending_append {
            self.take_pending_append(cx);
            return true;
        }
        false
    }

    /// A key's work (`run_key`), then the table the caret may have left is realigned.
    fn key_inner(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let handled = self.run_key(ks, cx);
        self.note_caret_moved(false, cx);
        handled
    }

    fn run_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        match key_action(ks) {
            Some(action) => self.run_action(action, cx),
            None => false,
        }
    }

    /// A menu row's Cut, Copy or Paste, done as its key does (the paste makes a link as Cmd+V's
    /// does).
    pub fn act(&mut self, action: KeyAction, cx: &mut Context<Self>) {
        if self.doc.is_none() {
            return;
        }
        if matches!(action, KeyAction::Copy | KeyAction::Cut | KeyAction::Paste) && self.table_clipboard(action, cx) {
            return;
        }
        if self.run_action(action, cx) && places_caret(action) {
            self.caret_placed = true;
        }
        self.note_caret_moved(false, cx);
    }

    /// The menu's Link (`md_format::link`) on one line: the selection or the word under the caret
    /// becomes a link to the address on the clipboard (`md_format::link_target`), or to nothing yet
    /// with the caret between the parentheses.
    pub fn insert_link(&mut self, cx: &mut Context<Self>) {
        let clip = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
        let base = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let target = md_format::link_target(&clip, &base, dirs_home().as_deref());
        let Some(doc) = self.doc.as_ref() else { return };
        let c = self.caret;
        let (lo, hi) = if c.anchor <= c.head { (doc.clamp(c.anchor), doc.clamp(c.head)) } else { (doc.clamp(c.head), doc.clamp(c.anchor)) };
        if lo.line != hi.line {
            return;
        }
        let line = doc.lines()[lo.line].clone();
        let byte = |ch: usize| line.char_indices().nth(ch).map_or(line.len(), |(b, _)| b);
        let e = md_format::link(&line, byte(lo.ch)..byte(hi.ch), target.as_deref());
        let n = lo.line;
        let ch = |s: &str, b: usize| s[..b.min(s.len())].chars().count();
        let (from, to) = (edit::Pos { line: n, ch: ch(&line, e.range.start) }, edit::Pos { line: n, ch: ch(&line, e.range.end) });
        let mut after = line.clone();
        after.replace_range(e.range.clone(), &e.text);
        let at = edit::Pos { line: n, ch: ch(&after, e.select.start) };
        self.caret_placed = true;
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(from, to, &e.text, now);
                doc.break_coalescing();
                edit::Caret::at(at)
            },
            cx,
        );
    }

    /// Whether the selection is empty or does not hold `p`: a right press there moves the caret
    /// before the menu opens, as a left press would.
    pub fn press_outside_selection(&self, p: edit::Pos) -> bool {
        let c = self.caret;
        let (lo, hi) = if c.anchor <= c.head { (c.anchor, c.head) } else { (c.head, c.anchor) };
        lo == hi || p < lo || p > hi
    }

    /// Whether anything is selected.
    pub fn has_selection(&self) -> bool {
        self.caret.anchor != self.caret.head
    }

    fn run_action(&mut self, action: KeyAction, cx: &mut Context<Self>) -> bool {
        if self.doc.is_none() {
            return false;
        }
        // A key the input method passed on ends its composition, keeping the text.
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        // A paragraph a click on the tail promised: an edit writes it first (before Enter reads
        // the structure), a move or an undo lets it go.
        if self.pending_append {
            match action {
                KeyAction::DeleteBack(_) | KeyAction::DeleteFwd(_) | KeyAction::Newline | KeyAction::BreakLine | KeyAction::Tab | KeyAction::ShiftTab | KeyAction::Cut | KeyAction::Paste => self.take_pending_append(cx),
                KeyAction::Copy | KeyAction::Save | KeyAction::Escape | KeyAction::ScrollHome | KeyAction::ScrollEnd => {}
                // The rendered view's page keys only scroll.
                KeyAction::Move(edit::Move::PageUp(_) | edit::Move::PageDown(_), _) if self.rendered() => {}
                KeyAction::Move(..) | KeyAction::SelectAll | KeyAction::Undo | KeyAction::Redo => self.pending_append = false,
            }
        }
        if self.rendered() && self.rendered_key(action, cx) {
            return true;
        }
        if matches!(action, KeyAction::DeleteBack(_) | KeyAction::DeleteFwd(_) | KeyAction::Newline | KeyAction::BreakLine | KeyAction::Tab | KeyAction::ShiftTab | KeyAction::Paste) {
            self.snap_for_edit();
        }
        // Enter and Shift+Enter in the rendered view go by the block (`rendered_break`); a block
        // with no rule of its own gets the editor's (a list's continuation, the fence rule).
        if self.rendered() && matches!(action, KeyAction::Newline | KeyAction::BreakLine) {
            let blocks = self.blocks.clone();
            let shift = action == KeyAction::BreakLine;
            self.edit(
                |doc, caret, now| match rendered_break(doc, &blocks, caret, shift, now) {
                    Some(c) => c,
                    None if shift => doc.insert(caret, "\n", now),
                    None => newline(doc, caret, &blocks, now),
                },
                cx,
            );
            return true;
        }
        // Backspace at an item's or a quote paragraph's text start takes its marker away.
        if self.rendered() && action == KeyAction::DeleteBack(edit::Unit::Grapheme) && self.caret.anchor == self.caret.head {
            let blocks = self.blocks.clone();
            let mut unmarked = false;
            self.edit(
                |doc, caret, now| match backspace_revealed(doc, &blocks, caret, now) {
                    Some(c) => {
                        unmarked = true;
                        c
                    }
                    None => caret,
                },
                cx,
            );
            if unmarked {
                return true;
            }
        }
        match action {
            KeyAction::Save => self.save_command(cx),
            // Escape undoes one thing at a time: the find bar closes, a selection collapses, and
            // in the rendered view the active block renders again until the caret is placed anew
            // (an edited table it was is realigned by `note_caret_moved` after the key).
            KeyAction::Escape => {
                if self.find.is_some() {
                    self.close_find(cx);
                } else if self.caret.anchor != self.caret.head {
                    self.caret.anchor = self.caret.head;
                    cx.notify();
                } else if self.rendered() {
                    self.caret_placed = false;
                    self.pending_append = false;
                    cx.notify();
                }
            }
            KeyAction::ScrollHome => {
                self.scroll.jump_to(0.0);
                cx.notify();
            }
            KeyAction::ScrollEnd => {
                self.scroll.jump_to(self.max_scroll());
                cx.notify();
            }
            KeyAction::Paste => {
                let item = cx.read_from_clipboard();
                // An image on the clipboard (no text with it) goes into a markdown file as a file.
                let image = item.as_ref().filter(|i| i.text().is_none()).and_then(|i| {
                    i.entries().iter().find_map(|e| match e {
                        gpui::ClipboardEntry::Image(image) => Some(image.clone()),
                        _ => None,
                    })
                });
                if let Some(image) = image.filter(|_| is_markdown(&self.path)) {
                    self.paste_image(&image, cx);
                } else if let Some(text) = item.and_then(|item| item.text()) {
                    // An address or a path pasted over a selection on one line makes a link.
                    let selected = self.doc.as_ref().map(|d| d.text_in(self.caret.anchor, self.caret.head)).unwrap_or_default();
                    let base = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
                    let text = md_format::paste_link(&selected, &text, &base, dirs_home().as_deref()).unwrap_or_else(|| normalize_paste(&text));
                    self.edit(|doc, caret, now| doc.insert(caret, &text, now), cx);
                }
            }
            KeyAction::Undo => self.undo(cx),
            KeyAction::Redo => self.redo(cx),
            KeyAction::Move(..) | KeyAction::SelectAll | KeyAction::Copy => {
                let rows = self.rows_visible.max(1);
                let Some(doc) = self.doc.as_mut() else { return false };
                let outcome = apply_action(doc, &mut self.caret, &[], action, rows, Instant::now());
                if let Outcome::Copy(text) = outcome {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                self.reveal_caret();
                cx.notify();
            }
            _ => {
                let rows = self.rows_visible.max(1);
                // Enter reads the structure (the fence rule), and in the rendered view so do Tab
                // and Shift+Tab (a table's cells).
                let reads_blocks = matches!(action, KeyAction::Newline | KeyAction::BreakLine) || (self.rendered() && matches!(action, KeyAction::Tab | KeyAction::ShiftTab));
                let blocks = if reads_blocks { self.blocks.clone() } else { Vec::new() };
                let mut outcome = Outcome::Nothing;
                self.edit(
                    |doc, caret, now| {
                        let mut caret = caret;
                        outcome = apply_action(doc, &mut caret, &blocks, action, rows, now);
                        caret
                    },
                    cx,
                );
                if let Outcome::Cut(text) = outcome {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
        }
        true
    }

    /// The keys the rendered view takes differently: the page keys scroll the list by its height,
    /// Home and End go to its ends, and Up and Down cross from block to block (`md_view::navigate`).
    /// `false` for every other key, which works as in the raw view.
    fn rendered_key(&mut self, action: KeyAction, cx: &mut Context<Self>) -> bool {
        match action {
            KeyAction::Move(edit::Move::PageDown(_) | edit::Move::PageUp(_), _) | KeyAction::ScrollHome | KeyAction::ScrollEnd => self.scroll_list(action),
            KeyAction::Move(m @ (edit::Move::Up | edit::Move::Down), select) => {
                if let Some(doc) = self.doc.as_mut() {
                    doc.break_coalescing();
                }
                if let Some(c) = self.reveal_vertical(m == edit::Move::Up, select) {
                    self.caret = c;
                } else {
                    let dir = if m == edit::Move::Up { md_view::Dir::Up } else { md_view::Dir::Down };
                    let Some(doc) = self.doc.as_ref() else { return false };
                    let mut c = md_view::navigate(doc, &self.blocks, self.caret, dir, select);
                    // Over a folded section, as if it were not there.
                    for _ in 0..self.blocks.len() {
                        if !self.is_hidden(md_blocks::block_at(&self.blocks, c.head.line)) {
                            break;
                        }
                        c = md_view::navigate(doc, &self.blocks, c, dir, select);
                    }
                    // Into a revealed list: not onto its first item's bullet.
                    let side = if m == edit::Move::Up { md_reveal::Dir::Left } else { md_reveal::Dir::Right };
                    let head = snap_pos(doc.lines(), &self.blocks, c.head, side);
                    self.caret = edit::Caret { head, anchor: if select { c.anchor } else { head }, goal_col: c.goal_col };
                }
                self.reveal_caret();
            }
            KeyAction::Move(m @ (edit::Move::LineStart | edit::Move::LineEnd), select) if self.caret_reveal().is_some() => {
                let Some(c) = self.reveal_row_edge(m == edit::Move::LineEnd, select) else { return false };
                if let Some(doc) = self.doc.as_mut() {
                    doc.break_coalescing();
                }
                self.caret = c;
                self.reveal_caret();
            }
            KeyAction::Move(m @ (edit::Move::Left | edit::Move::Right | edit::Move::WordLeft | edit::Move::WordRight | edit::Move::LineStart | edit::Move::LineEnd), select) => {
                let Some(doc) = self.doc.as_mut() else { return false };
                doc.break_coalescing();
                self.caret = move_snapped(doc, &self.blocks, self.caret, m, select);
                self.reveal_caret();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Cmd+S: writes the edits now; during a conflict it keeps mine, as the banner's button does.
    /// A clean document has nothing to write.
    pub fn save_command(&mut self, cx: &mut Context<Self>) {
        if self.conflict {
            self.resolve_conflict(true, cx);
        } else if self.dirty {
            self.save(cx);
        }
    }

    /// Scrolls the least that puts the caret's row inside the viewport; in the rendered view, the
    /// least that shows the caret's block.
    fn reveal_caret(&mut self) {
        if self.doc.is_none() {
            return;
        }
        if self.rendered() {
            self.reveal_in_list();
        }
        let row = self.row_of_line(self.caret.head.line) as f32;
        let rows = self.rows_visible.max(1) as f32;
        let top = self.scroll.target;
        let to = if row < top {
            row
        } else if row + 1.0 > top + rows {
            row + 1.0 - rows
        } else {
            return;
        };
        self.scroll.jump_to(to.clamp(0.0, self.max_scroll().max(0.0)));
    }

    /// Whether the rendered view shows this viewer: a markdown document in Rendered mode.
    pub fn rendered(&self) -> bool {
        self.mode == ViewMode::Rendered && self.doc.is_some()
    }

    /// The block holding the caret, which the rendered view shows raw.
    pub fn active_block(&self) -> usize {
        md_blocks::block_at(&self.blocks, self.caret.head.line)
    }

    /// Whether block `ix` is shown raw in the rendered view: every block the caret or the
    /// selection touches, once the caret has been placed (and not in a paragraph a click on the
    /// tail promised) and while the viewer has the focus (`focused`), and while the find bar is
    /// open the block holding the current hit.
    pub fn renders_raw(&self, ix: usize, focused: bool) -> bool {
        if self.doc.is_none() {
            return false;
        }
        let c = self.caret;
        let (lo, hi) = (c.head.min(c.anchor), c.head.max(c.anchor));
        let selected = md_blocks::block_at(&self.blocks, lo.line)..=md_blocks::block_at(&self.blocks, hi.line);
        (self.caret_placed && !self.pending_append && focused && selected.contains(&ix)) || self.find.as_ref().and_then(|f| f.hits.get(f.current)).is_some_and(|h| md_blocks::block_at(&self.blocks, h.line) == ix)
    }

    /// Cmd+B, Cmd+I and the menu's formats (`md_format::toggle`) on the selection, or at the caret,
    /// on one line: one undoable edit, after which the selection is the formatted text (or the
    /// caret keeps its place). A selection over more than one line, or a caret not yet placed in
    /// the rendered view, does nothing.
    pub fn format(&mut self, fmt: md_format::Format, cx: &mut Context<Self>) {
        if self.doc.is_none() || (self.rendered() && !self.caret_placed) {
            return;
        }
        self.finish_composition(cx);
        let Some(doc) = self.doc.as_ref() else { return };
        let c = self.caret;
        let (lo, hi) = if c.anchor <= c.head { (doc.clamp(c.anchor), doc.clamp(c.head)) } else { (doc.clamp(c.head), doc.clamp(c.anchor)) };
        if lo.line != hi.line {
            return;
        }
        let line = doc.lines()[lo.line].clone();
        let byte = |ch: usize| line.char_indices().nth(ch).map_or(line.len(), |(b, _)| b);
        let Some(e) = md_format::toggle(&line, byte(lo.ch)..byte(hi.ch), fmt) else { return };
        let n = lo.line;
        let ch = |s: &str, b: usize| s[..b.min(s.len())].chars().count();
        let (from, to) = (edit::Pos { line: n, ch: ch(&line, e.range.start) }, edit::Pos { line: n, ch: ch(&line, e.range.end) });
        let mut after = line.clone();
        after.replace_range(e.range.clone(), &e.text);
        let (a, b) = (edit::Pos { line: n, ch: ch(&after, e.select.start) }, edit::Pos { line: n, ch: ch(&after, e.select.end) });
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(from, to, &e.text, now);
                doc.break_coalescing();
                edit::Caret { head: b, anchor: a, goal_col: None }
            },
            cx,
        );
    }

    /// The headings and the hidden blocks, after an edit, a load or a fold.
    pub fn refresh_structure(&mut self) {
        let Some(doc) = self.doc.as_ref() else {
            self.heads.clear();
            self.hidden.clear();
            return;
        };
        self.heads = md_fold::headings(&self.blocks, doc.lines());
        let notes = md_blocks::footnotes(doc.lines(), &self.blocks);
        // Rows draw the numbers from these every frame, so nothing cached goes stale.
        if *self.notes != notes {
            self.notes = Rc::new(notes);
        }
        self.hidden = md_fold::hidden(&self.heads, &self.folded, self.blocks.len());
    }

    /// Whether block `ix` is hidden by a fold.
    pub fn is_hidden(&self, ix: usize) -> bool {
        self.hidden.get(ix).copied().unwrap_or(false)
    }

    fn folds_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_structure();
        let saved = self.folded.iter().map(|(heading, index)| workspace::Fold { heading: heading.clone(), index: *index }).collect();
        cx.emit(ViewerEvent::FoldsChanged(saved));
        cx.notify();
    }

    /// Folds or unfolds the heading at block `ix`. A caret its fold hides goes to the heading.
    pub fn toggle_fold(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(h) = self.heads.iter().find(|h| h.block == ix) else { return };
        let key = h.key();
        if !self.folded.remove(&key) {
            self.folded.insert(key);
        }
        self.folds_changed(cx);
        self.keep_caret_visible();
    }

    /// Fold All: every heading with something under it is folded.
    pub fn fold_all(&mut self, cx: &mut Context<Self>) {
        let n = self.blocks.len();
        self.folded = self.heads.iter().enumerate().filter(|(i, _)| !md_fold::section(&self.heads, *i, n).is_empty()).map(|(_, h)| h.key()).collect();
        self.folds_changed(cx);
        self.keep_caret_visible();
    }

    /// Unfold All.
    pub fn unfold_all(&mut self, cx: &mut Context<Self>) {
        self.folded.clear();
        self.folds_changed(cx);
    }

    /// Unfolds the headings hiding block `ix`.
    pub fn unfold_for(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.is_hidden(ix) {
            return;
        }
        for key in md_fold::hiding(&self.heads, &self.folded, ix, self.blocks.len()) {
            self.folded.remove(&key);
        }
        self.folds_changed(cx);
    }

    /// A caret a fold now hides goes to the nearest heading above it that shows.
    fn keep_caret_visible(&mut self) {
        let ix = self.active_block();
        if !self.is_hidden(ix) {
            return;
        }
        let Some(h) = self.heads.iter().rev().find(|h| h.block < ix && !self.is_hidden(h.block)) else { return };
        let line = self.blocks[h.block].lines.start;
        if let Some(doc) = self.doc.as_ref() {
            let end = md_blocks::pos_of_offset(&self.blocks[h.block], doc.lines(), usize::MAX);
            self.caret = edit::Caret::at(edit::Pos { line: end.line.max(line), ch: end.ch });
        }
    }

    /// The outline's click: the block a fold hides is shown, and the view scrolls to it.
    /// The folder pasted and dropped images go in (`md_links::image_dir`).
    fn image_dir(&self) -> PathBuf {
        crate::md_links::image_dir(&self.path, &self.image_destination)
    }

    /// A clipboard image written to the image folder at the next free `image-N` name, and
    /// `![image](path)` typed at the caret. A file that cannot be written leaves the document be.
    fn paste_image(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        use gpui::ImageFormat::*;
        let ext = match image.format {
            Png => "png",
            Jpeg => "jpg",
            Webp => "webp",
            Gif => "gif",
            Svg => "svg",
            Bmp => "bmp",
            Tiff => "tiff",
        };
        let dir = self.image_dir();
        let file = crate::md_links::image_name(&dir, ext, |p| p.exists());
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &image.bytes)) {
            eprintln!("arkos: cannot write the pasted image to {}: {e}", file.display());
            return;
        }
        let doc_dir = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let text = crate::md_links::file_markdown(&file, &file, &doc_dir);
        self.insert_paragraphs(vec![text], None, cx);
    }

    /// Files dropped from Finder before block `to` (the end of the document past the last), each on
    /// its own paragraph (`md_links::place_dropped`, `md_links::file_markdown`), as one undoable
    /// edit with the caret after them. An image that cannot be copied is left out.
    pub fn drop_files(&mut self, files: &[PathBuf], to: usize, cx: &mut Context<Self>) {
        if !is_markdown(&self.path) || self.read_only.is_some() || self.presenting {
            return;
        }
        let doc_dir = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let dir = self.image_dir();
        let mut items = Vec::new();
        for file in files {
            let (linked, copy) = crate::md_links::place_dropped(file, &doc_dir, &dir, |p| p.exists());
            if let Some(from) = copy {
                if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&from, &linked)) {
                    eprintln!("arkos: cannot copy {} to {}: {e}", from.display(), linked.display());
                    continue;
                }
            }
            items.push(crate::md_links::file_markdown(file, &linked, &doc_dir));
        }
        self.insert_paragraphs(items, Some(to), cx);
    }

    /// Paragraphs of their own (an image draws only as a paragraph of its own), as one undoable edit
    /// with the caret after the last: before block `to` (past the last block, at the end), or for
    /// `None` where the caret is (`paragraph_spot`).
    fn insert_paragraphs(&mut self, items: Vec<String>, to: Option<usize>, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_ref() else { return };
        if items.is_empty() {
            return;
        }
        let to = to.unwrap_or(usize::MAX);
        let caret = (to == usize::MAX).then_some(self.caret.head.line);
        let Some((at, text, last_line)) = paragraph_spot(doc.lines(), &self.blocks, to, caret, &items) else { return };
        self.caret_placed = true;
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(at, at, &text, now);
                doc.break_coalescing();
                let line = last_line.min(doc.lines().len().saturating_sub(1));
                edit::Caret::at(edit::Pos { line, ch: doc.lines()[line].chars().count() })
            },
            cx,
        );
    }

    /// Scrolls to the heading anchor `anchor` names (`md_links::heading_for`), unfolding it; `false`
    /// when no heading has it.
    pub fn go_to_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) -> bool {
        // `#^key` is a footnote reference: its note.
        if let Some(key) = anchor.strip_prefix('^') {
            let Some(&block) = self.notes.def_block.get(key) else { return false };
            self.reveal_block(block, cx);
            return true;
        }
        let texts: Vec<String> = self.heads.iter().map(|h| h.text.clone()).collect();
        let Some(i) = crate::md_links::heading_for(&texts, anchor) else { return false };
        let block = self.heads[i].block;
        self.reveal_block(block, cx);
        true
    }

    /// Presentation mode on or off (reading spec, section 4): the caret, any selection and every
    /// open menu go, so nothing shows revealed and nothing edits.
    pub fn set_presenting(&mut self, on: bool, cx: &mut Context<Self>) {
        self.presenting = on;
        self.caret_placed = false;
        self.selecting = false;
        if let Some(doc) = self.doc.as_ref() {
            self.caret = edit::Caret::at(doc.clamp(self.caret.head));
        }
        self.slash = None;
        self.lang = None;
        self.table_edit = false;
        self.table_pick = None;
        cx.notify();
    }

    /// Block `ix`'s index among the document's diagrams (mermaid code blocks), in document order.
    pub fn diagram_index(&self, ix: usize) -> usize {
        self.blocks[..ix.min(self.blocks.len())].iter().filter(|b| matches!(&b.kind, md_blocks::BlockKind::CodeBlock { info } if crate::mermaid::is_diagram_info(info))).count()
    }

    /// How block `ix`'s diagram shows.
    pub fn diagram_mode(&self, ix: usize) -> workspace::DiagramMode {
        self.diagram_modes.get(&self.diagram_index(ix)).copied().unwrap_or_default()
    }

    /// Shows block `ix`'s diagram as `mode`, saved for the file (`ViewerEvent::DiagramModesChanged`).
    pub fn set_diagram_mode(&mut self, ix: usize, mode: workspace::DiagramMode, cx: &mut Context<Self>) {
        let index = self.diagram_index(ix);
        if mode == workspace::DiagramMode::Diagram {
            self.diagram_modes.remove(&index);
        } else {
            self.diagram_modes.insert(index, mode);
        }
        let views = self.diagram_modes.iter().map(|(&index, &mode)| workspace::DiagramView { index, mode }).collect();
        cx.emit(ViewerEvent::DiagramModesChanged(views));
        cx.notify();
    }

    /// Block `ix`'s diagram one zoom step in or out about its centre, or back to fit (`None`), in a
    /// box `size` big.
    pub fn zoom_diagram(&mut self, ix: usize, step: Option<bool>, size: (f32, f32), cx: &mut Context<Self>) {
        let index = self.diagram_index(ix);
        let (zoom, pan) = self.diagram_zoom.get(&index).copied().unwrap_or((1.0, (0.0, 0.0)));
        match step {
            None => {
                self.diagram_zoom.remove(&index);
            }
            Some(in_) => {
                let to = crate::md_diagram::step(zoom, in_);
                self.diagram_zoom.insert(index, (to, crate::md_diagram::zoom_about(pan, size, zoom, to)));
            }
        }
        cx.notify();
    }

    /// A pan of block `ix`'s diagram: the press at `at` starts it, a move with the button down
    /// follows it, kept in its box `size` big.
    pub fn pan_diagram(&mut self, ix: usize, at: Point<Pixels>, start: bool, size: (f32, f32), cx: &mut Context<Self>) {
        let index = self.diagram_index(ix);
        let (zoom, pan) = self.diagram_zoom.get(&index).copied().unwrap_or((1.0, (0.0, 0.0)));
        if start {
            self.diagram_drag = Some((index, at, pan));
            return;
        }
        let Some((i, from, base)) = self.diagram_drag.filter(|(i, _, _)| *i == index) else { return };
        let moved = (base.0 + f32::from(at.x - from.x), base.1 + f32::from(at.y - from.y));
        self.diagram_zoom.insert(i, (zoom, crate::md_diagram::clamp(moved, size, zoom)));
        cx.notify();
    }

    /// Scrolls back to footnote `key`'s first reference.
    pub fn go_to_reference(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(&block) = self.notes.ref_block.get(key) {
            self.reveal_block(block, cx);
        }
    }

    /// Goes to heading `anchor` now if the document is loaded, else once it is.
    pub fn anchor_to(&mut self, anchor: String, cx: &mut Context<Self>) {
        if self.doc.is_some() && !self.heads.is_empty() {
            self.go_to_anchor(&anchor, cx);
        } else {
            self.pending_anchor = Some(anchor);
        }
    }

    pub fn reveal_block(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.unfold_for(ix, cx);
        self.list_state.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.0) });
        cx.notify();
    }

    /// Moves block `from` to before block `to` (`md_fold::move_block`), as one undoable edit, the
    /// caret at the moved block's start.
    pub fn move_block(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(m) = md_fold::move_block(doc.lines(), &self.blocks, from, to) else { return };
        let last = m.lines.end - 1;
        let end = edit::Pos { line: last, ch: doc.lines()[last].chars().count() };
        let text = m.text.join("\n");
        self.caret_placed = true;
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(edit::Pos { line: m.lines.start, ch: 0 }, end, &text, now);
                doc.break_coalescing();
                edit::Caret::at(doc.clamp(edit::Pos { line: m.at, ch: 0 }))
            },
            cx,
        );
    }

    /// After an edit or a caret move: the slash menu follows the text typed after the `/`, and
    /// closes when the `/` is gone, the caret leaves its line or goes before it, something is
    /// selected, or the text holds a space.
    fn refresh_slash(&mut self) {
        let Some(s) = self.slash.as_mut() else { return };
        let (Some(doc), c) = (self.doc.as_ref(), self.caret) else { return };
        let line: Vec<char> = doc.lines().get(s.line).map(|l| l.chars().collect()).unwrap_or_default();
        let ok = c.anchor == c.head && c.head.line == s.line && c.head.ch > s.col && line.get(s.col) == Some(&'/');
        let query: String = if ok { line[s.col + 1..c.head.ch.min(line.len())].iter().collect() } else { String::new() };
        if !ok || query.chars().any(char::is_whitespace) || query.chars().count() > 40 {
            self.slash = None;
            return;
        }
        if query != s.query {
            s.query = query;
            s.selected = 0;
            s.sub = None;
            s.sub_selected = 0;
        }
    }

    /// A key while the slash menu is open: Up and Down move the selection (in the open submenu when
    /// there is one), Enter and Tab run it, Right opens a submenu and Left closes it, Escape closes
    /// the menu and keeps the text. `false` for any other key, which the editor takes as usual.
    fn slash_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.slash.as_mut() else { return false };
        let m = &ks.modifiers;
        if m.platform || m.control || m.alt {
            return false;
        }
        let rows = s.rows();
        let sub_len = s.sub.and_then(|i| rows.get(i)).map_or(0, |e| e.submenu.len());
        match (ks.key.as_str(), m.shift) {
            ("escape", _) => self.slash = None,
            ("down" | "up", false) => {
                let (len, at) = if s.sub.is_some() { (sub_len, &mut s.sub_selected) } else { (rows.len(), &mut s.selected) };
                if len > 0 {
                    *at = if ks.key == "down" { (*at + 1) % len } else { (*at + len - 1) % len };
                }
            }
            ("right", false) if s.sub.is_none() && rows.get(s.selected).is_some_and(|e| !e.submenu.is_empty()) => {
                s.sub = Some(s.selected);
                s.sub_selected = 0;
            }
            ("left", false) if s.sub.is_some() => s.sub = None,
            ("enter" | "tab", false) => {
                let picked = match s.sub {
                    Some(i) => rows.get(i).and_then(|e| e.submenu.get(s.sub_selected)).map(|e| e.command),
                    None => rows.get(s.selected).map(|e| e.command),
                };
                match picked {
                    Some(cmd) => self.run_slash(cmd, cx),
                    // "No Results": the menu closes and the key does what it always does.
                    None => {
                        self.slash = None;
                        return false;
                    }
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Runs a slash menu entry (`md_slash::apply`) in place of the `/` and the text after it, as one
    /// undoable edit; an image asks for its file first.
    pub fn run_slash(&mut self, cmd: md_slash::Command, cx: &mut Context<Self>) {
        let Some(s) = self.slash.take() else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        let end = self.caret.head.ch;
        if cmd == md_slash::Command::Image {
            self.pick_image(s.line, s.col, end, cx);
            return;
        }
        let Some(e) = md_slash::apply(cmd, doc.lines(), s.line, s.col, end) else { return };
        self.apply_slash_edit(e, cx);
    }

    fn apply_slash_edit(&mut self, e: md_slash::SlashEdit, cx: &mut Context<Self>) {
        let p = |(line, ch): (usize, usize)| edit::Pos { line, ch };
        self.caret_placed = true;
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(p(e.from), p(e.to), &e.text, now);
                doc.break_coalescing();
                edit::Caret::at(doc.clamp(p(e.caret)))
            },
            cx,
        );
    }

    /// The slash menu's Image: the system's file picker, then `![name](path)` in place of the
    /// slash, the path relative to the document's folder when the image is inside it. A picker
    /// cancelled, or a document changed under the slash meanwhile, changes nothing.
    fn pick_image(&mut self, line: usize, col: usize, end: usize, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        let before = self.doc.as_ref().and_then(|d| d.lines().get(line).cloned());
        let base = self.path.parent().map(Path::to_path_buf).unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(file) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |v, cx| {
                let Some(doc) = v.doc.as_ref() else { return };
                if doc.lines().get(line) != before.as_ref() {
                    return;
                }
                let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let path = file.strip_prefix(&base).map(|r| r.to_string_lossy().into_owned()).unwrap_or_else(|_| file.to_string_lossy().into_owned());
                if let Some(e) = md_slash::image_edit(doc.lines(), line, col, end, &name, &path) {
                    v.apply_slash_edit(e, cx);
                }
            });
        })
        .detach();
    }

    /// The table holding the caret in the rendered view, when it parses: its block, its source and
    /// the caret's cell (its `row` a line of the block).
    pub fn table_cell(&self) -> Option<(usize, String, md_table::CellPos)> {
        if !self.rendered() || !self.caret_placed {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let ix = md_blocks::block_at(&self.blocks, self.caret.head.line);
        let block = self.blocks.get(ix).filter(|b| b.kind == md_blocks::BlockKind::Table)?;
        let source = md_view::block_source(doc.lines(), block);
        md_table::parse(&source)?;
        let pos = md_table::cell_at(&source, self.caret.head.line - block.lines.start, self.caret.head.ch)?;
        Some((ix, source, pos))
    }

    /// The caret to a table cell: its text's start (or end, `edit`, to type after it), selecting
    /// it, or editing it with `edit`.
    pub fn select_cell(&mut self, ix: usize, pos: md_table::CellPos, edit: bool, cx: &mut Context<Self>) {
        let (Some(doc), Some(block)) = (self.doc.as_mut(), self.blocks.get(ix)) else { return };
        let source = md_view::block_source(doc.lines(), block);
        let Some(range) = md_table::cell_range(&source, pos) else { return };
        doc.break_coalescing();
        let ch = if edit { range.end } else { range.start };
        self.caret = edit::Caret::at(edit::Pos { line: block.lines.start + pos.row, ch });
        self.caret_placed = true;
        self.selecting = false;
        self.table_edit = edit;
        self.table_pick = None;
        self.lang = None;
        self.note_caret_moved(false, cx);
        cx.notify();
    }

    /// A table's row, column or all of it picked by its edge: its first cell selected and the
    /// pick drawn, for Delete and the menu.
    pub fn pick_table(&mut self, ix: usize, pick: Pick, cx: &mut Context<Self>) {
        let (row, cell) = match pick {
            Pick::Row(r) => (md_table::line_of_row(r), 0),
            Pick::Column(c) => (0, c),
            Pick::All => (0, 0),
        };
        self.select_cell(ix, md_table::CellPos { row, cell }, false, cx);
        self.table_pick = Some((ix, pick));
        cx.notify();
    }

    /// Applies a table operation (`md_table::apply`) to block `ix` as one undoable edit, then
    /// selects cell `then` (grid row and column) when the table is still there.
    fn table_op(&mut self, ix: usize, op: md_table::Op, then: Option<(usize, usize)>, cx: &mut Context<Self>) {
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return };
        let source = md_view::block_source(doc.lines(), block);
        let Some(new) = md_table::apply(&source, &op) else { return };
        let start = block.lines.start;
        let last = block.lines.end.min(doc.lines().len()) - 1;
        let end = edit::Pos { line: last, ch: doc.lines()[last].chars().count() };
        self.table_pick = None;
        self.table_edit = false;
        self.edit(
            |doc, _caret, now| {
                doc.break_coalescing();
                doc.replace_range(edit::Pos { line: start, ch: 0 }, end, &new, now);
                doc.break_coalescing();
                edit::Caret::at(doc.clamp(edit::Pos { line: start, ch: 0 }))
            },
            cx,
        );
        match then {
            Some((r, c)) if md_table::parse(&new).is_some() => self.select_cell(ix, md_table::CellPos { row: md_table::line_of_row(r), cell: c }, false, cx),
            // The table went: the caret stays where it was, which starts no table now.
            _ => {}
        }
    }

    /// The grid row and column of the caret's cell, and the table's row and column counts.
    fn table_shape(source: &str, pos: md_table::CellPos) -> Option<(usize, usize, usize, usize)> {
        let t = md_table::parse(source)?;
        Some((md_table::row_of_line(pos.row)?, pos.cell, t.rows.len() + 1, t.header.len()))
    }

    /// The table menu's actions on the selected cell's row and column.
    pub fn table_menu(&mut self, action: TableAction, cx: &mut Context<Self>) {
        use md_table::Op;
        let Some((ix, source, pos)) = self.table_cell() else { return };
        let Some((r, c, rows, _cols)) = Self::table_shape(&source, pos) else { return };
        let (op, then) = match action {
            TableAction::RowAbove => (Op::InsertRow(r), Some((r, c))),
            TableAction::RowBelow => (Op::InsertRow(r + 1), Some((r + 1, c))),
            TableAction::ColumnLeft => (Op::InsertColumn(c), Some((r, c))),
            TableAction::ColumnRight => (Op::InsertColumn(c + 1), Some((r, c + 1))),
            TableAction::DeleteRow => (Op::DeleteRow(r), Some((r.min(rows.saturating_sub(2)), c))),
            TableAction::DeleteColumn => (Op::DeleteColumn(c), Some((r, c.saturating_sub(1)))),
            TableAction::DeleteTable => (Op::DeleteTable, None),
            TableAction::Align(a) => (Op::Align(c, a), Some((r, c))),
        };
        self.table_op(ix, op, then, cx);
    }

    /// A "+" strip: a column at the table's right, or a row at its bottom, its first cell selected.
    pub fn table_grow(&mut self, ix: usize, column: bool, cx: &mut Context<Self>) {
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return };
        let Some(t) = md_table::parse(&md_view::block_source(doc.lines(), block)) else { return };
        let (rows, cols) = (t.rows.len() + 1, t.header.len());
        if column {
            self.table_op(ix, md_table::Op::InsertColumn(cols), Some((0, cols)), cx);
        } else {
            self.table_op(ix, md_table::Op::InsertRow(rows), Some((rows, 0)), cx);
        }
    }

    /// A click on a table cell (its source line `row` in the block, column `cell`, char `ch` into
    /// its text): selects the cell, or, on a second click or in the cell being edited, edits it
    /// with the caret there.
    pub fn press_table_cell(&mut self, ix: usize, row: usize, cell: usize, ch: usize, clicks: usize, cx: &mut Context<Self>) {
        let pos = md_table::CellPos { row, cell };
        let editing_here = self.table_edit && self.table_cell().is_some_and(|(b, _, p)| b == ix && p == pos);
        if clicks < 2 && !editing_here {
            self.select_cell(ix, pos, false, cx);
            return;
        }
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return };
        let source = md_view::block_source(doc.lines(), block);
        let Some(range) = md_table::cell_range(&source, pos) else { return };
        let ch = if clicks >= 2 && !editing_here { range.end } else { (range.start + ch).min(range.end) };
        self.caret = edit::Caret::at(edit::Pos { line: block.lines.start + row, ch });
        self.caret_placed = true;
        self.selecting = false;
        self.table_edit = true;
        self.table_pick = None;
        cx.notify();
    }

    /// Cut, Copy and Paste on a selected cell (or a pick): its text to the clipboard, or the
    /// clipboard's first line into it. `false` when no cell is selected.
    pub fn table_clipboard(&mut self, action: KeyAction, cx: &mut Context<Self>) -> bool {
        if self.table_edit {
            return false;
        }
        let Some((ix, source, pos)) = self.table_cell() else { return false };
        let Some((r, c, _, _)) = Self::table_shape(&source, pos) else { return false };
        let text = || md_table::cell_range(&source, pos).map(|range| source.split('\n').nth(pos.row).unwrap_or("").chars().skip(range.start).take(range.len()).collect::<String>()).unwrap_or_default();
        match action {
            KeyAction::Copy => cx.write_to_clipboard(ClipboardItem::new_string(text())),
            KeyAction::Cut => {
                cx.write_to_clipboard(ClipboardItem::new_string(text()));
                self.table_op(ix, md_table::Op::Set(r, c, String::new()), Some((r, c)), cx);
            }
            KeyAction::Paste => {
                let clip = cx.read_from_clipboard().and_then(|i| i.text()).unwrap_or_default();
                let line = clip.lines().next().unwrap_or("").to_string();
                self.table_op(ix, md_table::Op::Set(r, c, line), Some((r, c)), cx);
            }
            _ => return false,
        }
        true
    }

    /// A key while the caret is in a table of the rendered view (tables spec, section 1), as
    /// `table_move` decides it. `false` for a key it leaves to the editor (typing, and moves inside
    /// an editing cell's text).
    fn table_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        use md_table::Op;
        let Some((ix, source, pos)) = self.table_cell() else { return false };
        let Some((r, c, rows, cols)) = Self::table_shape(&source, pos) else { return false };
        let range = md_table::cell_range(&source, pos).unwrap_or(0..0);
        let cell = TableCell { r, c, rows, cols, ch: self.caret.head.ch, text: range.clone(), editing: self.table_edit };
        let at = |r: usize, c: usize| md_table::CellPos { row: md_table::line_of_row(r), cell: c };
        match table_move(ks, &cell) {
            TableMove::Pass => return false,
            TableMove::Stay => {}
            TableMove::Select(r, c) => self.select_cell(ix, at(r, c), false, cx),
            TableMove::Edit => self.select_cell(ix, pos, true, cx),
            TableMove::Caret(ch) => {
                self.caret = edit::Caret::at(edit::Pos { line: self.caret.head.line, ch });
                cx.notify();
            }
            TableMove::AppendRow(c) => self.table_op(ix, Op::InsertRow(rows), Some((rows, c)), cx),
            TableMove::Leave(up) => {
                let Some(doc) = self.doc.as_ref() else { return true };
                let dir = if up { md_view::Dir::Up } else { md_view::Dir::Down };
                self.caret = md_view::navigate(doc, &self.blocks, self.caret, dir, false);
                self.table_edit = false;
                self.table_pick = None;
                self.note_caret_moved(false, cx);
                cx.notify();
            }
            TableMove::Clipboard(action) => return self.table_clipboard(action, cx),
            TableMove::Clear => {
                let pick = self.table_pick.filter(|(b, _)| *b == ix).map(|(_, p)| p);
                self.table_op(ix, Op::Clear(picked_cells(pick, r, c, rows, cols)), Some((r, c)), cx);
            }
            TableMove::Escape if self.table_pick.is_some() => {
                self.table_pick = None;
                cx.notify();
            }
            TableMove::Escape => {
                self.caret_placed = false;
                self.note_caret_moved(false, cx);
                cx.notify();
            }
        }
        true
    }

    /// Opens a code block's language list under its pill (closing it when it is open there).
    pub fn open_lang_picker(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.slash = None;
        self.lang = match &self.lang {
            Some(l) if l.block == ix => None,
            _ => Some(LangPicker { block: ix, query: String::new(), selected: 0 }),
        };
        self.lang_at = None;
        cx.notify();
    }

    /// Sets the code block's language (`md_code::set_language`) as one undoable edit, and closes
    /// the list.
    pub fn set_language(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(l) = self.lang.take() else { return };
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(l.block)) else { return };
        let Some((line, text)) = crate::md_code::set_language(doc.lines(), block, id) else { return };
        let end = doc.lines()[line].chars().count();
        let caret = self.caret;
        self.edit_elsewhere(
            |doc, _c, now| {
                doc.break_coalescing();
                doc.replace_range(edit::Pos { line, ch: 0 }, edit::Pos { line, ch: end }, &text, now);
                doc.break_coalescing();
                edit::Caret { head: doc.clamp(caret.head), anchor: doc.clamp(caret.anchor), goal_col: None }
            },
            cx,
        );
    }

    /// A key while the language list is open: Up and Down choose, Enter sets, Escape closes,
    /// Delete takes a character off the search. `false` for keys it leaves alone.
    fn lang_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(l) = self.lang.as_mut() else { return false };
        let m = &ks.modifiers;
        if m.platform || m.control || m.alt {
            return false;
        }
        let n = crate::md_code::filter_languages(&l.query).len();
        match ks.key.as_str() {
            "escape" => self.lang = None,
            "down" if n > 0 => l.selected = (l.selected + 1) % n,
            "up" if n > 0 => l.selected = (l.selected + n - 1) % n,
            "backspace" => {
                l.query.pop();
                l.selected = 0;
            }
            "enter" | "tab" => {
                let id = crate::md_code::filter_languages(&l.query).get(l.selected).map(|(id, _)| *id);
                match id {
                    Some(id) => self.set_language(id, cx),
                    None => self.lang = None,
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Copy on a code block's header: its code on the clipboard, and "Copied" on the button for a
    /// moment.
    pub fn copy_code(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(crate::md_code::copy_text(doc.lines(), block)));
        self.copied = Some((ix, Instant::now()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// A click on a code block's header selects the whole block, fences included, so Delete takes it
    /// away.
    pub fn select_block(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return };
        let last = md_blocks::pos_of_offset(block, doc.lines(), usize::MAX);
        self.lang = None;
        self.caret = edit::Caret { head: last, anchor: edit::Pos { line: block.lines.start, ch: 0 }, goal_col: None };
        self.caret_placed = true;
        self.selecting = false;
        self.note_caret_moved(false, cx);
        cx.notify();
    }

    /// A revealed text was laid out: when it holds the open slash menu's `/`, its window point is
    /// recorded for the menu. `true` when that point moved, so the frame is drawn again with the
    /// menu where the `/` now is.
    pub fn note_slash_layout(&mut self, r: &md_view::RevealLayout) -> bool {
        let (Some(s), Some(doc)) = (self.slash.as_ref(), self.doc.as_ref()) else { return false };
        let Some(block) = self.blocks.get(r.block).filter(|b| b.lines == r.lines && b.lines.contains(&s.line)) else { return false };
        let offset = md_blocks::offset_of_pos(block, doc.lines(), edit::Pos { line: s.line, ch: s.col });
        if offset < r.covers.start || offset > r.covers.end {
            return false;
        }
        let Some(at) = r.layout.position_for_index(r.display.display_of(offset)) else { return false };
        let new = Some((at, r.layout.line_height()));
        let moved = self.slash_at != new;
        self.slash_at = new;
        moved
    }

    /// Block `ix`'s element tree: the cached one when its rendering is cached (made there the
    /// first time), else made now.
    pub fn block_tree(&mut self, ix: usize) -> Option<Rc<md_reveal::BlockTree>> {
        let (doc, block) = (self.doc.as_ref()?, self.blocks.get(ix)?);
        let make = || Rc::new(md_reveal::BlockTree::new(&md_view::block_source(doc.lines(), block)));
        match self.block_cache.get_mut(ix) {
            Some(Some(r)) => Some(r.tree.get_or_insert_with(make).clone()),
            _ => Some(make()),
        }
    }

    /// Whether the active block `ix` is drawn revealed (`md_reveal::reveals`) rather than as its
    /// source. Only a paragraph, heading, list or quote has its tree looked at.
    pub fn reveals(&mut self, ix: usize) -> bool {
        let Some(kind) = self.blocks.get(ix).map(|b| b.kind.clone()) else { return false };
        md_reveal::kind_reveals(&kind) && self.block_tree(ix).is_some_and(|t| md_reveal::reveals(&kind, &t))
    }

    /// Where the user is in block `ix` (`focus_of`): the caret once it is placed, the find bar's
    /// hits, the input method's marked text.
    pub fn focus_for(&self, ix: usize) -> md_reveal::Focus {
        let (Some(doc), Some(block)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return md_reveal::Focus::default() };
        let (hits, current) = match &self.find {
            Some(f) => (f.hits.as_slice(), f.hits.get(f.current).copied()),
            None => (&[][..], None),
        };
        let caret = if self.caret_placed && !self.pending_append { self.caret } else { edit::Caret::at(edit::Pos { line: usize::MAX, ch: 0 }) };
        focus_of(doc.lines(), block, caret, hits, current, self.marked.as_ref())
    }

    /// The lines a raw block shows: from its first line to its last line with text, or further
    /// down to the caret, the selection's ends or the marked text when they are on the blank lines
    /// after it.
    pub fn raw_lines(&self, ix: usize) -> Range<usize> {
        let (Some(doc), Some(b)) = (self.doc.as_ref(), self.blocks.get(ix)) else { return 0..0 };
        let lines = doc.lines();
        let mut last = md_blocks::pos_of_offset(b, lines, usize::MAX).line;
        let ends = [self.caret.head, self.caret.anchor].into_iter().chain(self.marked.iter().flat_map(|m| [m.start, m.end]));
        for p in ends.filter(|p| b.lines.contains(&p.line)) {
            last = last.max(p.line);
        }
        b.lines.start..(last + 1).min(lines.len()).max(b.lines.start)
    }

    /// Scrolls the rendered view the least that shows the caret's row: the caret's block when it
    /// is above the view, and within a block taller than the view, the caret's line rather than
    /// the block's top or bottom (which `scroll_to_reveal_item` would show). The caret's place in
    /// its block comes from the block's raw layout of the last frame, else a row per line.
    fn reveal_in_list(&mut self) {
        let ix = self.active_block();
        let Some(block) = self.blocks.get(ix) else { return };
        let line = self.caret.head.line;
        let line_h = self.font_size * self.line_height;
        // In a revealed text: the caret's row from its layout, below the row's top.
        let revealed_y = self.reveal_point_of(self.caret.head).zip(self.list_state.bounds_for_item(ix)).map(|((at, _), item)| f32::from(at.y - item.top()).max(0.0));
        let caret_y = revealed_y.or_else(|| self
            .raw_blocks
            .iter()
            .find(|b| b.lines.start == block.lines.start && line >= b.lines.start)
            .map(|b| {
                // A line past the layout's last one (just after Enter on it) is a row per line
                // below the layout's rows.
                let row = match b.row_tops.get(line - b.lines.start).filter(|_| line < b.lines.end) {
                    Some(&r) => r,
                    None => b.row_tops.last().copied().unwrap_or(0) + (line - b.lines.end.max(b.lines.start)),
                };
                row as f32 * b.raw.line_h
            }))
            .unwrap_or(line.saturating_sub(block.lines.start) as f32 * line_h);
        let viewport = self.list_state.viewport_bounds();
        let view_h = f32::from(viewport.size.height);
        if view_h <= 0.0 {
            self.list_state.scroll_to_reveal_item(ix);
            return;
        }
        let top = self.list_state.logical_scroll_top();
        let offset = f32::from(top.offset_in_item);
        if ix < top.item_ix || (ix == top.item_ix && caret_y < offset) {
            // Above the view: the block's top when the caret's row fits below it, else the least
            // scroll, which puts the caret's row at the top.
            let to = if ix < top.item_ix && caret_y + line_h <= view_h { 0.0 } else { caret_y };
            self.list_state.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(to) });
            return;
        }
        if ix == top.item_ix {
            if caret_y + line_h > offset + view_h {
                self.list_state.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(caret_y + line_h - view_h) });
            }
            return;
        }
        // Below the top row: where the block starts on screen, from the list's last layout of it
        // or of the row above it (an edited block is measured again on the next frame).
        let item_top = self.list_state.bounds_for_item(ix).map(|b| b.top()).or_else(|| self.list_state.bounds_for_item(ix - 1).map(|b| b.bottom()));
        match item_top {
            Some(t) => {
                let over = f32::from(t - viewport.top()) + caret_y + line_h - view_h;
                if over > 0.0 {
                    self.list_state.scroll_by(px(over));
                }
            }
            None => self.list_state.scroll_to_reveal_item(ix),
        }
    }

    /// A click on the tail of the rendered view: the caret goes to a new paragraph after the last
    /// block. The blank lines it needs (`md_view::append_paragraph`) are written with the first
    /// edit there (`pending_append`), so a click alone leaves the file as it is; a document that
    /// already ends in them takes the caret on its last line at once.
    pub fn append_paragraph(&mut self, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        let Some(doc) = self.doc.as_mut() else { return };
        doc.break_coalescing();
        let lines = doc.lines();
        let last = lines.len() - 1;
        self.pending_append = md_view::breaks_for_paragraph(lines) > 0;
        let ch = if self.pending_append { lines[last].chars().count() } else { 0 };
        self.caret = edit::Caret::at(edit::Pos { line: last, ch });
        self.caret_placed = true;
        self.selecting = false;
        if !self.pending_append {
            self.reveal_caret();
        }
        self.note_caret_moved(false, cx);
        cx.notify();
    }

    /// Writes the blank lines a click on the tail promised, as their own undo step, before the
    /// first edit in the new paragraph.
    fn take_pending_append(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.pending_append) {
            self.run_edit(|doc, _caret, now| md_view::append_paragraph(doc, now), cx);
        }
    }

    /// Ends a composition with its marked text kept: the marked step is taken back and the text
    /// inserted as typing, so it joins the typing before it.
    fn finish_composition(&mut self, cx: &mut Context<Self>) {
        let (marked, compose) = (self.marked.take(), self.compose.take());
        let (Some(m), Some(c), Some(doc)) = (marked, compose, self.doc.as_ref()) else { return };
        let text = doc.text_in(m.start, m.end);
        self.edit_raw(
            |doc, _caret, now| {
                if c.step {
                    doc.retract();
                }
                doc.insert(c.before, &text, now)
            },
            cx,
        );
    }

    /// The input method's field: the active block, from its first line's start to its last
    /// line's end.
    fn field(&self) -> Option<(edit::Pos, edit::Pos)> {
        let doc = self.doc.as_ref()?;
        let b = self.blocks.get(md_blocks::block_at(&self.blocks, self.caret.head.line))?;
        let from = doc.clamp(edit::Pos { line: b.lines.start, ch: 0 });
        let to = doc.clamp(edit::Pos { line: b.lines.end.saturating_sub(1).max(b.lines.start), ch: usize::MAX });
        Some((from, to))
    }

    /// A position as a UTF-16 offset in the field, clamped into it.
    fn field_offset(&self, p: edit::Pos) -> usize {
        match (self.doc.as_ref(), self.field()) {
            (Some(doc), Some((from, to))) => doc.pos_to_utf16(from, doc.clamp(p).clamp(from, to)),
            _ => 0,
        }
    }

    /// A UTF-16 offset in the field as a position, clamped to the field's end.
    fn field_pos(&self, units: usize) -> edit::Pos {
        match (self.doc.as_ref(), self.field()) {
            (Some(doc), Some((from, to))) => doc.utf16_to_pos(from, units).min(to),
            _ => edit::Pos::default(),
        }
    }

    /// The display row under `y`, mapped to the line it shows (a deleted row of the inline diff
    /// gives the line it sits before).
    fn line_at_y(&self, y: Pixels, layout: &RawLayout) -> usize {
        let rel = f32::from(y - layout.bounds.origin.y) / layout.line_h + self.scroll.current;
        let row = (rel.floor().max(0.0) as usize).min(self.row_count().saturating_sub(1));
        match &self.diff {
            Some(d) if self.inline_diff => diff::line_at_row(d, &diff::display_rows(self.line_count(), d, true), row),
            _ => row,
        }
    }

    /// The revealed text of the last frame holding the caret's head, with its block, the head's
    /// display byte and the text's displayed rows.
    fn caret_reveal(&self) -> Option<(&md_view::RevealLayout, &md_blocks::Block, usize, Vec<Range<usize>>)> {
        let doc = self.doc.as_ref()?;
        let ix = self.active_block();
        let texts: Vec<(&md_view::RevealLayout, &md_blocks::Block)> = self.live_reveals().filter(|(r, _)| r.block == ix).collect();
        let block = texts.first()?.1;
        let offset = md_blocks::offset_of_pos(block, doc.lines(), doc.clamp(self.caret.head));
        let covers: Vec<Range<usize>> = texts.iter().map(|(r, _)| r.covers.clone()).collect();
        let (r, block) = texts[pick_cover(&covers, offset)?];
        let at = r.display.display_of(offset);
        let layout = &r.layout;
        // An empty text is laid out as a zero-width space, one row.
        let rows = if r.display.text.is_empty() { vec![0..0] } else { rows_by_y(&r.display.text, |b| layout.position_for_index(b).map_or(0.0, |p| f32::from(p.y))) };
        Some((r, block, at, rows))
    }

    /// Up or Down by displayed row inside the revealed text holding the caret, aiming at the x
    /// the last such move started from; `None` on its first or last row, or with no revealed
    /// text, for the move between blocks.
    fn reveal_vertical(&mut self, up: bool, select: bool) -> Option<edit::Caret> {
        let head = self.caret.head;
        let (r, block, at, rows) = self.caret_reveal()?;
        let x_of = |b: usize| r.layout.position_for_index(b).map_or(0.0, |p| f32::from(p.x));
        let goal = match self.reveal_goal {
            Some((p, x)) if p == head => x,
            _ => x_of(at),
        };
        let dir = if up { md_reveal::Dir::Up } else { md_reveal::Dir::Down };
        let to = md_reveal::vertical(&r.display.text, &rows, x_of, at, dir, goal)?;
        let lines = self.doc.as_ref()?.lines();
        let p = md_blocks::pos_of_offset(block, lines, r.display.source_of(to));
        self.reveal_goal = Some((p, goal));
        Some(edit::Caret { head: p, anchor: if select { self.caret.anchor } else { p }, goal_col: None })
    }

    /// Cmd+Left or Cmd+Right inside the revealed text holding the caret: the start or end of the
    /// caret's displayed row.
    fn reveal_row_edge(&self, end: bool, select: bool) -> Option<edit::Caret> {
        let (r, block, at, rows) = self.caret_reveal()?;
        let (a, b) = md_reveal::row_edges(&rows, at);
        let lines = self.doc.as_ref()?.lines();
        let p = md_blocks::pos_of_offset(block, lines, r.display.source_of(if end { b } else { a }));
        Some(edit::Caret { head: p, anchor: if select { self.caret.anchor } else { p }, goal_col: None })
    }

    /// Before an edit at the caret with nothing selected: the caret moved off a hidden marker
    /// (`snap_pos`), so what is typed lands after the bullet the caret is drawn after.
    fn snap_for_edit(&mut self) {
        if !self.rendered() || self.caret.anchor != self.caret.head || self.marked.is_some() {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let p = snap_pos(doc.lines(), &self.blocks, self.caret.head, md_reveal::Dir::Here);
        if p != self.caret.head {
            self.caret = edit::Caret::at(p);
        }
    }

    /// The revealed texts of the last frame whose block is still where it was, with that block.
    fn live_reveals(&self) -> impl Iterator<Item = (&md_view::RevealLayout, &md_blocks::Block)> {
        self.reveal_blocks.iter().filter_map(|r| self.blocks.get(r.block).filter(|b| b.lines == r.lines).map(|b| (r, b)))
    }

    /// The window point where position `p` is drawn in a revealed text of the last frame, with
    /// that text's line height.
    fn reveal_point_of(&self, p: edit::Pos) -> Option<(Point<Pixels>, Pixels)> {
        let doc = self.doc.as_ref()?;
        let ix = md_blocks::block_at(&self.blocks, p.line);
        let block = self.blocks.get(ix).filter(|b| b.lines.contains(&p.line))?;
        let offset = md_blocks::offset_of_pos(block, doc.lines(), doc.clamp(p));
        let texts: Vec<&md_view::RevealLayout> = self.live_reveals().filter(|(r, _)| r.block == ix).map(|(r, _)| r).collect();
        let covers: Vec<Range<usize>> = texts.iter().map(|r| r.covers.clone()).collect();
        let r = texts.get(pick_cover(&covers, offset)?)?;
        Some((r.layout.position_for_index(r.display.display_of(offset))?, r.layout.line_height()))
    }

    /// The document position under a window point, through the last layout.
    pub fn pos_at_point(&self, at: Point<Pixels>, window: &mut Window) -> Option<edit::Pos> {
        if self.rendered() {
            // The raw block or revealed text at that height, else the first one below it, else
            // the last one.
            let doc = self.doc.as_ref()?;
            enum Laid<'a> {
                Raw(&'a md_view::BlockLayout),
                Reveal(&'a md_view::RevealLayout, &'a md_blocks::Block),
            }
            let mut laid: Vec<(f32, f32, Laid)> = self.raw_blocks.iter().map(|b| (f32::from(b.raw.bounds.origin.y), f32::from(b.bottom()), Laid::Raw(b))).collect();
            laid.extend(self.live_reveals().map(|(r, b)| {
                let bounds = r.layout.bounds();
                (f32::from(bounds.top()), f32::from(bounds.bottom()), Laid::Reveal(r, b))
            }));
            laid.sort_by(|a, b| a.0.total_cmp(&b.0));
            let spans: Vec<(f32, f32)> = laid.iter().map(|(t, b, _)| (*t, *b)).collect();
            return match &laid.get(pick_by_y(&spans, f32::from(at.y))?)?.2 {
                Laid::Raw(b) => Some(b.pos_at(doc.lines(), at, window)),
                Laid::Reveal(r, block) => Some(md_blocks::pos_of_offset(block, doc.lines(), r.display.source_of(md_view::index_near(&r.layout, at)))),
            };
        }
        let (doc, layout) = (self.doc.as_ref()?, self.last_bounds.as_ref()?);
        let n = self.line_at_y(at.y, layout).min(doc.lines().len() - 1);
        let line = &doc.lines()[n];
        let (expanded, shaped) = element::shape_raw(window, line, layout);
        let col = element::col_for_x(&shaped, &expanded, at.x - layout.text_left + px(self.scroll_x));
        Some(edit::Pos { line: n, ch: element::ch_for_col(line, col) })
    }

    /// The window x of a position and the top of its row, through the last layout.
    fn point_of_pos(&self, p: edit::Pos, window: &mut Window) -> Option<Point<Pixels>> {
        if self.rendered() {
            // Only a raw block or a revealed text on screen knows where its text is.
            if let Some((at, _)) = self.reveal_point_of(p) {
                return Some(at);
            }
            let doc = self.doc.as_ref()?;
            return self.raw_blocks.iter().find(|b| b.lines.contains(&p.line))?.point_of(doc.lines(), doc.clamp(p), window);
        }
        let (doc, layout) = (self.doc.as_ref()?, self.last_bounds.as_ref()?);
        let p = doc.clamp(p);
        let line = &doc.lines()[p.line];
        let (expanded, shaped) = element::shape_raw(window, line, layout);
        let x = layout.text_left - px(self.scroll_x) + element::x_for_col(&shaped, &expanded, element::display_col(line, p.ch));
        let y = layout.bounds.origin.y + px((self.row_of_line(p.line) as f32 - self.scroll.current) * layout.line_h);
        Some(gpui::point(x, y))
    }

    /// A left-button press in the text: the caret goes there, Shift extends the selection, and a
    /// drag from here selects.
    pub fn mouse_down(&mut self, at: Point<Pixels>, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        let Some(p) = self.pos_at_point(at, window) else { return };
        self.place_caret(p, extend, cx);
    }

    /// Puts the caret at `p` (Shift keeps the anchor, so the selection extends) for a press in the
    /// text of either view, and starts a drag selection. The typing step ends there.
    pub fn place_caret(&mut self, p: edit::Pos, extend: bool, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        self.lang = None;
        let rendered = self.rendered();
        let Some(doc) = self.doc.as_mut() else { return };
        doc.break_coalescing();
        let p = doc.clamp(p);
        // A press that maps onto a hidden fence line (a click on a diagram) lands in the code.
        let p = if rendered { snap_pos(doc.lines(), &self.blocks, p, md_reveal::Dir::Here) } else { p };
        let anchor = if extend { doc.clamp(self.caret.anchor) } else { p };
        self.caret = edit::Caret { head: p, anchor, goal_col: None };
        self.selecting = true;
        self.caret_placed = true;
        self.pending_append = false;
        self.note_caret_moved(false, cx);
        cx.notify();
    }


    /// Block `ix`'s index among the document's `<details>` sections, in document order
    /// (`section_index`).
    pub fn details_index(&self, ix: usize) -> usize {
        section_index(&self.blocks, ix)
    }

    /// Whether `<details>` block `ix`, whose summary is `summary` and whose tag says `starts_open`,
    /// is shown open (`section_shown_open`).
    pub fn details_open(&self, ix: usize, summary: &str, starts_open: bool) -> bool {
        section_shown_open(&self.open_details, summary, self.details_index(ix), starts_open)
    }

    /// Opens or closes `<details>` block `ix` (`flip_section`) and tells `Root`, which saves the
    /// file's section states in the workspace. The document is not touched.
    pub fn toggle_details(&mut self, ix: usize, summary: &str, starts_open: bool, cx: &mut Context<Self>) {
        let index = self.details_index(ix);
        flip_section(&mut self.open_details, summary, index, starts_open);
        crate::telemetry::md(crate::telemetry::MdFeature::Details);
        cx.emit(ViewerEvent::DetailsToggled(sections_saved(&self.open_details)));
        cx.notify();
    }

    /// After the caret moved (a key, a press, a click on the tail, an undo or a redo): in the
    /// rendered view, `next_entry` decides whether the table block `table_entry` holds was left
    /// with its text changed since the snapshot, and that table's pipes are realigned
    /// (`realign_on_leave`, one undo step); then `table_entry` takes the snapshot `next_entry`
    /// gives. `refresh` (an undo or a redo) takes a new snapshot of the table the caret is in even
    /// when it never left, so the snapshot always holds the text at the last entry or history
    /// step, and undoing back to the original text and leaving realigns nothing. A caret that is
    /// not placed (after Escape, where the grid draws again) has left the table. Outside the
    /// rendered view nothing is realigned and the entry is forgotten.
    fn note_caret_moved(&mut self, refresh: bool, cx: &mut Context<Self>) {
        self.refresh_slash();
        if self.table_cell().is_none() {
            self.table_edit = false;
            self.table_pick = None;
        }
        // The caret never stands in a folded section: landing there unfolds it.
        if self.rendered() && self.caret_placed {
            let ix = self.active_block();
            self.unfold_for(ix, cx);
        }
        if !self.rendered() {
            self.table_entry = None;
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let (realign, next) = next_entry(&self.blocks, doc.lines(), self.caret, self.table_entry.take(), refresh, !self.caret_placed);
        if let Some((range, entry)) = realign {
            crate::telemetry::md(crate::telemetry::MdFeature::TableEdit);
            self.edit_elsewhere(
                |doc, caret, now| {
                    realign_on_leave(doc, range, &entry, now);
                    // The realigned lines are not the caret's, but a caret that was placed from
                    // the old lines stays on the document.
                    edit::Caret { head: doc.clamp(caret.head), anchor: doc.clamp(caret.anchor), goal_col: caret.goal_col }
                },
                cx,
            );
        }
        // The realign kept every line count, and the snapshot is of another block.
        self.table_entry = next;
    }

    /// The mouse moved with the left button down: a drag that began in the text moves the head.
    pub fn mouse_drag(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let Some(p) = self.pos_at_point(at, window) else { return };
        self.drag_to(p, cx);
    }

    /// A drag selection reached `p`. The raw view scrolls to follow it; the rendered view does
    /// not, since the head is under the mouse and revealing its block could move a tall block
    /// away from it.
    pub fn drag_to(&mut self, p: edit::Pos, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        let Some(doc) = self.doc.as_ref().filter(|_| self.selecting) else { return };
        let p = doc.clamp(p);
        if p != self.caret.head {
            self.caret.head = p;
            self.caret.goal_col = None;
            if !self.rendered() {
                self.reveal_caret();
            }
            cx.notify();
        }
    }

    /// Whether a drag selection is in progress.
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    pub fn mouse_up(&mut self) {
        self.selecting = false;
    }
}

/// Text input from the system: typing, dead keys and input method compositions. The field the
/// input method sees is the active block's text (`field`), with UTF-16 offsets from its first
/// line's start. Typing goes through `Document::insert`, so it coalesces into undo steps; marked
/// text is written with `replace_range` and retracted until it is committed, so a composition
/// leaves one step.
impl EntityInputHandler for Viewer {
    fn text_for_range(&mut self, range: Range<usize>, adjusted_range: &mut Option<Range<usize>>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<String> {
        let (from, _) = self.field()?;
        let doc = self.doc.as_ref()?;
        let (a, b) = (self.field_pos(range.start), self.field_pos(range.end.max(range.start)));
        *adjusted_range = Some(doc.pos_to_utf16(from, a)..doc.pos_to_utf16(from, b));
        Some(doc.text_in(a, b))
    }

    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
        self.field()?;
        let c = self.caret;
        let (lo, hi) = if c.anchor <= c.head { (c.anchor, c.head) } else { (c.head, c.anchor) };
        Some(UTF16Selection { range: self.field_offset(lo)..self.field_offset(hi), reversed: c.head < c.anchor })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let m = self.marked.clone()?;
        Some(self.field_offset(m.start)..self.field_offset(m.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition(cx);
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        if self.field().is_none() {
            return;
        }
        // While a code block's language list is open, typing searches it.
        if let Some(l) = self.lang.as_mut() {
            l.query.push_str(text);
            l.selected = 0;
            cx.notify();
            return;
        }
        // In a table: typing into a selected cell replaces its text and edits it; in an editing
        // cell a pipe goes in escaped.
        if self.marked.is_none() {
            if let Some((ix, source, pos)) = self.table_cell() {
                if !self.table_edit {
                    if let Some((r, c, _, _)) = Self::table_shape(&source, pos) {
                        self.table_op(ix, md_table::Op::Set(r, c, text.to_string()), Some((r, c)), cx);
                        if let Some((_, _, p)) = self.table_cell() {
                            self.select_cell(ix, p, true, cx);
                        }
                    }
                    return;
                }
                let escaped = md_table::escape(text);
                self.edit_raw(|doc, caret, now| doc.insert(caret, &escaped, now), cx);
                return;
            }
        }
        let range = if self.place_for_input(cx) { None } else { range };
        if range.is_none() {
            self.snap_for_edit();
        }
        let target = range.map(|r| (self.field_pos(r.start), self.field_pos(r.end.max(r.start))));
        if let (Some(m), Some(c)) = (self.marked.clone(), self.compose) {
            // The usual commit: the marked text becomes `text`, typed where the composition began.
            if target.is_none_or(|t| t == (m.start, m.end)) {
                self.marked = None;
                self.compose = None;
                self.edit_raw(
                    |doc, _caret, now| {
                        if c.step {
                            doc.retract();
                        }
                        doc.insert(c.before, text, now)
                    },
                    cx,
                );
                return;
            }
            // Somewhere else: the composition is kept as it stands (its text unchanged, so the
            // target computed above still holds), then the range is replaced.
            self.finish_composition(cx);
        }
        let c = self.caret;
        let selection = if c.anchor <= c.head { (c.anchor, c.head) } else { (c.head, c.anchor) };
        match target {
            Some((a, b)) if (a, b) != selection => self.edit_raw(|doc, _caret, now| edit::Caret::at(doc.replace_range(a, b, text, now)), cx),
            _ => self.edit_raw(|doc, caret, now| doc.insert(caret, text, now), cx),
        }
        // A `/` where a word could start opens the slash menu (the rendered view only).
        if text == "/" && self.rendered() && self.caret.anchor == self.caret.head && self.caret.head.ch > 0 {
            let (line, col) = (self.caret.head.line, self.caret.head.ch - 1);
            if self.doc.as_ref().is_some_and(|d| slash_opens(d.lines(), &self.blocks, line, col)) {
                self.slash = Some(Slash { line, col, query: String::new(), selected: 0, sub: None, sub_selected: 0 });
                self.slash_at = None;
                cx.notify();
            }
        }
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, new_text: &str, new_selected_range: Option<Range<usize>>, _window: &mut Window, cx: &mut Context<Self>) {
        if self.presenting {
            return;
        }
        if self.field().is_none() {
            return;
        }
        let range = if self.place_for_input(cx) { None } else { range };
        if range.is_none() {
            self.snap_for_edit();
        }
        let explicit = range.map(|r| (self.field_pos(r.start), self.field_pos(r.end.max(r.start))));
        let at_range = |(a, b): (edit::Pos, edit::Pos)| edit::Caret { head: b, anchor: a, goal_col: None };
        // Where the marked text goes, in the text before the composition, and whether the marked
        // step on top is to be taken back first.
        let (before, retract) = match (self.marked.clone(), self.compose) {
            (Some(m), Some(c)) if explicit.is_none_or(|t| t == (m.start, m.end)) => (c.before, c.step),
            (Some(_), Some(_)) => {
                self.finish_composition(cx);
                (explicit.map_or(self.caret, at_range), false)
            }
            _ => (explicit.map_or(self.caret, at_range), false),
        };
        let (lo, hi) = if before.anchor <= before.head { (before.anchor, before.head) } else { (before.head, before.anchor) };
        let mut wrote = false;
        self.edit_raw(
            |doc, caret, now| {
                if retract {
                    doc.retract();
                }
                if lo == hi && new_text.is_empty() {
                    return if retract { edit::Caret::at(lo) } else { caret };
                }
                wrote = true;
                edit::Caret::at(doc.replace_range(lo, hi, new_text, now))
            },
            cx,
        );
        if new_text.is_empty() {
            // Everything composed was deleted: the composition is over.
            self.marked = None;
            self.compose = None;
            return;
        }
        let end = self.caret.head;
        self.marked = Some(lo..end);
        self.compose = Some(Compose { before, step: wrote });
        if let (Some(r), Some(doc)) = (new_selected_range, self.doc.as_ref()) {
            let (a, b) = (doc.utf16_to_pos(lo, r.start).min(end), doc.utf16_to_pos(lo, r.end.max(r.start)).min(end));
            self.caret = edit::Caret { head: b, anchor: a, goal_col: None };
        }
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _element_bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let (a, b) = (self.field_pos(range_utf16.start), self.field_pos(range_utf16.end.max(range_utf16.start)));
        let line_h = match self.reveal_point_of(a) {
            Some((_, h)) if self.rendered() => h,
            _ => px(if self.rendered() { self.raw_blocks.first()?.raw.line_h } else { self.last_bounds.as_ref()?.line_h }),
        };
        let start = self.point_of_pos(a, window)?;
        let width = if b.line == a.line { self.point_of_pos(b, window)?.x - start.x } else { px(0.0) };
        Some(Bounds { origin: start, size: gpui::size(width.max(px(1.0)), line_h) })
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        let p = self.pos_at_point(point, window)?;
        Some(self.field_offset(p))
    }
}

/// A viewer's file, read and highlighted: call it on the background executor. `None` when the
/// fingerprint is still `known` (nothing to reload). Over `limit`, or with a line over
/// `MAX_LINE_BYTES`, the text is shown plain; binaries and unreadable files say so. With
/// `with_diff`, text not shown plain for its size is diffed against its worktree's HEAD.
///
/// A markdown file is never highlighted and has no size limit but `MAX_PLAIN_BYTES`: clean UTF-8
/// with one kind of line ending, in a file the user may write, becomes a document whose lines
/// (tabs kept, the empty last line after a final break kept) are the content's, its blocks split
/// here too; anything else is plain text, read-only, saying why.
pub fn read_file(path: &Path, known: Option<(u128, u64)>, highlighter: Option<&Highlighter>, limit: usize, with_diff: bool) -> Option<Loaded> {
    let fingerprint = fingerprint(path);
    if known.is_some() && fingerprint == known {
        return None;
    }
    let plain = |lines: Vec<String>| {
        let spans = vec![Vec::new(); lines.len()];
        Content::Text { lines: Arc::new(lines), spans }
    };
    let mut doc = None;
    let mut read_only = None;
    let mut coloured = Some(false);
    let (content, why_plain) = match read_capped(path) {
        Err(e) => (Content::Unreadable(e.to_string()), None),
        Ok(Capped::TooLarge(len)) => (Content::TooLarge(len), None),
        Ok(Capped::Binary) => (Content::Binary, None),
        Ok(Capped::Bytes(bytes)) if is_markdown(path) => match markdown_document(&bytes).and_then(|d| if writable(path) { Ok(d) } else { Err(ReadOnlyWhy::NotWritable) }) {
            Ok(d) => {
                let lines = d.shared_lines();
                doc = Some(d);
                (Content::Text { lines, spans: Vec::new() }, None)
            }
            Err(why) => {
                read_only = Some(why);
                (plain(split_lines(&String::from_utf8_lossy(&bytes))), None)
            }
        },
        Ok(Capped::Bytes(bytes)) => match classify(&bytes, limit) {
            Classified::Binary => (Content::Binary, None),
            Classified::TooBig => (plain(split_lines(&String::from_utf8_lossy(&bytes))), Some(Plain::TooBig)),
            Classified::Text(text) => {
                let lines = split_lines(&text);
                match highlighter {
                    _ if lines.iter().any(|l| l.len() > MAX_LINE_BYTES) => (plain(lines), Some(Plain::LongLine)),
                    Some(h) => {
                        let syntax = h.syntax_for(path, &lines);
                        coloured = Some(syntax.is_some_and(|s| s.name != "Plain Text"));
                        let spans = h.highlight_with(syntax, &lines);
                        (Content::Text { lines: Arc::new(lines), spans }, None)
                    }
                    // The grammars are still loading: plain for now, coloured when they land.
                    None => {
                        coloured = None;
                        (plain(lines), None)
                    }
                }
            }
        },
    };
    let diff = match (&content, &doc) {
        // The base is split the same way, so the document's text is too (tabs expanded, no empty
        // last line).
        (_, Some(d)) if with_diff => diff_of(path, &split_lines(&d.to_text())),
        (Content::Text { lines, .. }, None) if with_diff && why_plain != Some(Plain::TooBig) => diff_of(path, lines),
        _ => None,
    };
    let blocks = doc.as_ref().map_or_else(Vec::new, |d| md_blocks::split(d.lines()));
    Some(Loaded { content, fingerprint, plain: why_plain, diff, doc, read_only, blocks, coloured })
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

/// The rendered view's row count for `blocks` blocks: one per block and the tail after them.
pub fn list_rows(blocks: usize) -> usize {
    if blocks == 0 {
        0
    } else {
        blocks + 1
    }
}

/// The block holding line `line` with its text and tree, when an active block there is drawn
/// revealed (`md_reveal::reveals`).
fn revealed_block<'a>(lines: &[String], blocks: &'a [md_blocks::Block], line: usize) -> Option<(&'a md_blocks::Block, String, md_reveal::BlockTree)> {
    let block = blocks.get(md_blocks::block_at(blocks, line)).filter(|b| b.lines.contains(&line) && md_reveal::kind_reveals(&b.kind))?;
    let text = md_view::block_source(lines, block);
    let tree = md_reveal::BlockTree::new(&text);
    md_reveal::reveals(&block.kind, &tree).then_some((block, text, tree))
}

/// `p` moved off a hidden marker of a revealed block (`md_reveal::snap`), onward in `dir`; any
/// other position as it is.
pub fn snap_pos(lines: &[String], blocks: &[md_blocks::Block], p: edit::Pos, dir: md_reveal::Dir) -> edit::Pos {
    use md_reveal::Dir as D;
    // A fenced code block's fence lines never show: off the opening one Left goes above the block
    // and anything else into its code; off the closing one Right goes below it and anything else
    // to the code's end.
    if let Some(block) = blocks.get(md_blocks::block_at(blocks, p.line)).filter(|b| b.lines.contains(&p.line)) {
        let code = crate::md_code::code_lines(lines, block);
        if let Some(f) = crate::md_code::fence(lines, block).filter(|_| !code.is_empty()) {
            let line_end = |n: usize| edit::Pos { line: n, ch: lines[n].chars().count() };
            if p.line == f.open {
                return match dir {
                    D::Left | D::Up if p.line > 0 => line_end(p.line - 1),
                    _ => edit::Pos { line: code.start, ch: 0 },
                };
            }
            if Some(p.line) == f.close {
                return match dir {
                    D::Right | D::Down if p.line + 1 < lines.len() => edit::Pos { line: p.line + 1, ch: 0 },
                    _ => line_end(code.end - 1),
                };
            }
            return p;
        }
    }
    let Some((block, text, tree)) = revealed_block(lines, blocks, p.line) else { return p };
    let at = md_blocks::offset_of_pos(block, lines, p);
    let to = md_reveal::snap(&text, &tree.markers, at, dir);
    if to == at {
        p
    } else {
        md_blocks::pos_of_offset(block, lines, to)
    }
}

/// A sideways caret move in the rendered view: the plain move, then the head moved off any hidden
/// marker it landed in, onward (Left and Option+Left go on to the line above, the rest to the
/// marker's end), so the caret never stands unseen on a bullet.
pub fn move_snapped(doc: &edit::Document, blocks: &[md_blocks::Block], caret: edit::Caret, m: edit::Move, select: bool) -> edit::Caret {
    let moved = doc.move_by(caret, m, select);
    let dir = if matches!(m, edit::Move::Left | edit::Move::WordLeft) { md_reveal::Dir::Left } else { md_reveal::Dir::Right };
    let head = snap_pos(doc.lines(), blocks, moved.head, dir);
    edit::Caret { head, anchor: if select { moved.anchor } else { head }, goal_col: moved.goal_col }
}

/// Backspace with the caret, and nothing selected, at the first visible offset of a list item
/// or of a paragraph in a quote of a revealed block: the marker goes (`md_reveal::unmark`) as one
/// edit, and the caret stays at the text's start. `None` anywhere else, for the ordinary delete.
pub fn backspace_revealed(doc: &mut edit::Document, blocks: &[md_blocks::Block], caret: edit::Caret, now: Instant) -> Option<edit::Caret> {
    if caret.anchor != caret.head {
        return None;
    }
    let lines = doc.lines();
    let (block, text, tree) = revealed_block(lines, blocks, caret.head.line)?;
    let un = md_reveal::unmark(&text, &tree.nodes, md_blocks::offset_of_pos(block, lines, caret.head))?;
    let (from, to) = (md_blocks::pos_of_offset(block, lines, un.range.start), md_blocks::pos_of_offset(block, lines, un.range.end));
    doc.break_coalescing();
    doc.replace_range(from, to, &un.text, now);
    doc.break_coalescing();
    // The caret `caret` bytes into the new text: down a line per line break, then along.
    let lead = &un.text[..un.caret];
    let at = match lead.rfind('\n') {
        Some(i) => edit::Pos { line: from.line + lead.matches('\n').count(), ch: lead[i + 1..].chars().count() },
        None => edit::Pos { line: from.line, ch: from.ch + lead.chars().count() },
    };
    Some(edit::Caret::at(doc.clamp(at)))
}

/// The displayed rows of a laid-out text: the runs of its char boundaries drawn at one height
/// (`y_of`), each as its first and last boundary.
pub fn rows_by_y(text: &str, y_of: impl Fn(usize) -> f32) -> Vec<Range<usize>> {
    let mut rows: Vec<Range<usize>> = Vec::new();
    let mut last_y = f32::NAN;
    for b in (0..=text.len()).filter(|&b| text.is_char_boundary(b)) {
        let y = y_of(b);
        match rows.last_mut() {
            Some(r) if (y - last_y).abs() < 0.5 => r.end = b,
            _ => rows.push(b..b),
        }
        last_y = y;
    }
    rows
}

/// A quote line's prefix: its indent and each `>` with the space after it.
fn quote_prefix(line: &str) -> &str {
    let b = line.as_bytes();
    let (mut i, mut end) = (0, 0);
    loop {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if i >= b.len() || b[i] != b'>' {
            return &line[..end];
        }
        i += 1;
        if i < b.len() && b[i] == b' ' {
            i += 1;
        }
        end = i;
    }
}

/// Enter (`shift` false) or Shift+Enter in the rendered view, by the block holding the caret
/// (spec `2026-10-30-typing-rules-design.md`, sections 3 and 4): a new paragraph in a paragraph or
/// a heading, a new quoted paragraph in a quote (an empty quote line ends the quote); a hard break
/// in a paragraph or an item; out of a quote or, from its last code line, out of a code block.
/// `None` for the editor's own rules (a list's next item, the fence rule, a plain break).
pub fn rendered_break(doc: &mut edit::Document, blocks: &[md_blocks::Block], caret: edit::Caret, shift: bool, now: Instant) -> Option<edit::Caret> {
    use md_blocks::BlockKind as K;
    if caret.anchor != caret.head {
        return None;
    }
    let at = doc.clamp(caret.head);
    let block = blocks.get(md_blocks::block_at(blocks, at.line)).filter(|b| b.lines.contains(&at.line))?;
    let line = doc.lines()[at.line].clone();
    let len = line.chars().count();
    let line_end = edit::Pos { line: at.line, ch: len };
    let last_text = md_blocks::pos_of_offset(block, doc.lines(), usize::MAX).line;
    let down = |doc: &edit::Document, by: usize, ch: usize| edit::Caret::at(doc.clamp(edit::Pos { line: at.line + by, ch }));
    // On a blank line after the block's text, either key adds a line.
    if at.line > last_text && line.trim().is_empty() {
        doc.insert(edit::Caret::at(at), "\n", now);
        return Some(down(doc, 1, 0));
    }
    match (&block.kind, shift) {
        (K::Paragraph | K::Heading(_), false) | (K::Heading(_), true) => {
            doc.insert(edit::Caret::at(at), "\n\n", now);
            Some(down(doc, 2, 0))
        }
        (K::Paragraph, true) => {
            doc.insert(edit::Caret::at(at), "\\\n", now);
            Some(down(doc, 1, 0))
        }
        (K::List, true) => {
            // The continuation indent: the content column of the item holding the line.
            let col = (block.lines.start..=at.line).rev().find_map(|n| md_blocks::list_item_columns(&doc.lines()[n])).map_or(0, |(_, c)| c);
            doc.insert(edit::Caret::at(at), &format!("\\\n{}", " ".repeat(col)), now);
            Some(down(doc, 1, col))
        }
        (K::Quote, false) => {
            let prefix = quote_prefix(&line).to_string();
            if line[prefix.len()..].trim().is_empty() {
                // The bare `>` line Enter left above goes too, so a blank line ends the quote.
                let prev = at.line.checked_sub(1).filter(|&p| block.lines.contains(&p)).filter(|&p| {
                    let l = &doc.lines()[p];
                    l.contains('>') && l[quote_prefix(l).len()..].trim().is_empty()
                });
                let from = edit::Pos { line: prev.unwrap_or(at.line), ch: 0 };
                doc.replace_range(from, line_end, if prev.is_some() { "\n" } else { "" }, now);
                return Some(edit::Caret::at(edit::Pos { line: at.line, ch: 0 }));
            }
            let bare = prefix.trim_end();
            let ch = prefix.chars().count();
            doc.insert(edit::Caret::at(at), &format!("\n{bare}\n{prefix}"), now);
            Some(down(doc, 2, ch))
        }
        (K::Quote, true) if at.line >= last_text => {
            doc.insert(edit::Caret::at(line_end), "\n\n", now);
            Some(down(doc, 2, 0))
        }
        (K::Quote, true) => {
            let prefix = quote_prefix(&line).to_string();
            let rest_empty = at.ch >= len;
            doc.insert(edit::Caret::at(at), &if rest_empty { "\n\n\n".to_string() } else { format!("\n\n\n\n{prefix}") }, now);
            Some(down(doc, 2, 0))
        }
        (K::CodeBlock { .. }, true) => {
            let lines = doc.lines();
            let fenced = |l: &str| {
                let t = l.trim_start();
                t.starts_with("```") || t.starts_with("~~~")
            };
            let close = last_text;
            if close <= block.lines.start || !fenced(&lines[block.lines.start]) || !fenced(&lines[close]) || at.line + 1 < close {
                return None;
            }
            let end = edit::Pos { line: close, ch: lines[close].chars().count() };
            doc.insert(edit::Caret::at(end), "\n\n", now);
            Some(edit::Caret::at(doc.clamp(edit::Pos { line: close + 2, ch: 0 })))
        }
        _ => None,
    }
}

/// The user's home folder, for a `~` path pasted as a link.
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Of laid-out texts given as `(top, bottom)` in window order, the one a point at `y` maps
/// through: the first whose bottom is below `y`, else the last.
pub fn pick_by_y(spans: &[(f32, f32)], y: f32) -> Option<usize> {
    spans.iter().position(|&(_, bottom)| y < bottom).or(spans.len().checked_sub(1))
}

/// Of the parts of a block's text that revealed texts show (`covers`, in order), the one an
/// offset is drawn in: the last that starts at or before it and reaches it.
pub fn pick_cover(covers: &[Range<usize>], offset: usize) -> Option<usize> {
    covers.iter().rposition(|c| c.start <= offset && offset <= c.end)
}

/// Where the user is in `block`, in bytes of its text (`md_blocks::offset_of_pos`): the caret's
/// head and anchor when they are in it (the head is also `caret`), the selection clipped to it,
/// the find hits in it with the current one marked and its two ends among the offsets (so the
/// block reveals what the hit is in), and the marked text.
pub fn focus_of(lines: &[String], block: &md_blocks::Block, caret: edit::Caret, hits: &[Hit], current: Option<Hit>, marked: Option<&Range<edit::Pos>>) -> md_reveal::Focus {
    let inside = |p: edit::Pos| block.lines.contains(&p.line);
    let at = |p: edit::Pos| md_blocks::offset_of_pos(block, lines, p);
    let len = md_view::block_source(lines, block).len();
    let line_at = |line: usize| at(edit::Pos { line, ch: 0 });
    let mut focus = md_reveal::Focus::default();
    for p in [caret.head, caret.anchor] {
        if inside(p) && !focus.offsets.contains(&at(p)) {
            focus.offsets.push(at(p));
        }
    }
    focus.caret = inside(caret.head).then(|| at(caret.head));
    let (lo, hi) = if caret.anchor <= caret.head { (caret.anchor, caret.head) } else { (caret.head, caret.anchor) };
    if lo != hi && lo.line < block.lines.end && hi.line >= block.lines.start {
        let from = if lo.line < block.lines.start { 0 } else { at(lo) };
        let to = if hi.line >= block.lines.end { len } else { at(hi) };
        focus.selection = (from < to).then_some(from..to);
    }
    for h in hits.iter().filter(|h| block.lines.contains(&h.line)) {
        let base = line_at(h.line);
        let is_current = current == Some(*h);
        focus.hits.push((base + h.start..base + h.end, is_current));
        if is_current {
            focus.offsets.extend([base + h.start, base + h.end]);
        }
    }
    focus.marked = marked.filter(|m| inside(m.start) && inside(m.end)).map(|m| at(m.start)..at(m.end));
    focus
}

/// The rendered view's rows for a freshly loaded document: every row new, the tail after the
/// blocks, every cache entry empty. Never `ListState::reset`, which would jump to the top.
fn reset_rows(blocks: &[md_blocks::Block], cache: &mut Vec<Option<md_view::Rendered>>, list: &ListState) {
    list.splice(0..list.item_count(), list_rows(blocks.len()));
    *cache = std::iter::repeat_with(|| None).take(blocks.len()).collect();
}

/// After an edit from `old_lines` to `new_lines`: the blocks it changed are split again and
/// spliced in, and the rendered rows and their cache follow the same splice, so the list keeps
/// one row per block and the tail.
fn splice_rows(blocks: &mut Vec<md_blocks::Block>, cache: &mut Vec<Option<md_view::Rendered>>, list: &ListState, old_lines: &[String], new_lines: &[String]) {
    // This needs the old text, so the edit copied the lines once: the price of comparing blocks
    // by their text.
    let new_blocks = md_blocks::split(new_lines);
    let (range, count) = md_blocks::changed_blocks(blocks, old_lines, &new_blocks, new_lines);
    let tail = range.start + count;
    blocks.splice(range.clone(), new_blocks[range.start..tail].iter().cloned());
    // Never `reset`, which would jump to the top. gpui moves a top row inside the splice to the
    // splice's start at its top, which would jump a long block being typed into back to its first
    // line, so the row and the offset into it are kept.
    let top = list.logical_scroll_top();
    list.splice(range.clone(), count);
    if range.contains(&top.item_ix) && count > 0 {
        list.scroll_to(ListOffset { item_ix: top.item_ix.min(range.start + count - 1), offset_in_item: top.offset_in_item });
    }
    cache.splice(range, std::iter::repeat_with(|| None).take(count));
    // The blocks after the change kept their kind and text but may have moved.
    for (block, new) in blocks[tail..].iter_mut().zip(&new_blocks[tail..]) {
        block.lines = new.lines.clone();
    }
}

/// The document position `ch` chars into the text of cell `pos` of table block `block`: the cell's
/// start (`md_table::cell_start`, its first character or an empty cell's caret place) plus `ch`,
/// at most the line's end. `None` when the block's source has no such row.
/// The caret's table cell as the keys see it: its grid row and column, the table's row and
/// column counts, the caret's char on its line and the cell's text there, and whether it is
/// being edited.
struct TableCell {
    r: usize,
    c: usize,
    rows: usize,
    cols: usize,
    ch: usize,
    text: Range<usize>,
    editing: bool,
}

/// What a key does in a table (tables spec, section 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableMove {
    /// The editor takes the key.
    Pass,
    /// The key is taken and does nothing (an edge, a pipe Delete would reach).
    Stay,
    /// Select this cell (grid row, column).
    Select(usize, usize),
    /// Edit the selected cell, the caret after its text.
    Edit,
    /// The caret to this char of the editing cell's line.
    Caret(usize),
    /// A new row at the bottom, its cell in this column selected.
    AppendRow(usize),
    /// Leave the table, up or down.
    Leave(bool),
    Clipboard(KeyAction),
    /// Clear the selected cell or the pick.
    Clear,
    /// Drop the pick, or leave the table with no caret.
    Escape,
}

fn table_move(ks: &Keystroke, t: &TableCell) -> TableMove {
    use TableMove::*;
    let m = &ks.modifiers;
    let (cmd, shift) = (m.platform, m.shift);
    if m.control || (m.alt && !t.editing) {
        return Pass;
    }
    let (r, c) = (t.r, t.c);
    let down = || if r + 1 < t.rows { Select(r + 1, c) } else { AppendRow(c) };
    let tab = |back: bool| match (back, c + 1 < t.cols, r + 1 < t.rows) {
        (false, true, _) => Select(r, c + 1),
        (false, false, true) => Select(r + 1, 0),
        (false, false, false) => AppendRow(0),
        (true, _, _) if c > 0 => Select(r, c - 1),
        (true, _, _) if r > 0 => Select(r - 1, t.cols - 1),
        (true, _, _) => Stay,
    };
    let up_or_leave = || if r > 0 { Select(r - 1, c) } else { Leave(true) };
    let down_or_leave = || if r + 1 < t.rows { Select(r + 1, c) } else { Leave(false) };
    let key = ks.key.as_str();
    if t.editing {
        return match (key, cmd) {
            ("left", true) | ("home", _) => Caret(t.text.start),
            ("right", true) | ("end", _) => Caret(t.text.end),
            (_, true) => Pass,
            ("left", _) if t.ch <= t.text.start && !m.alt => Stay,
            ("right", _) if t.ch >= t.text.end && !m.alt => Stay,
            (_, _) if m.alt => Pass,
            ("up", _) => up_or_leave(),
            ("down", _) => down_or_leave(),
            ("enter", _) if !shift => down(),
            ("tab", _) => tab(shift),
            ("escape", _) => Select(r, c),
            ("backspace", _) if t.ch <= t.text.start => Stay,
            ("delete", _) if t.ch >= t.text.end => Stay,
            _ => Pass,
        };
    }
    match (key, cmd) {
        ("c", true) if !shift => Clipboard(KeyAction::Copy),
        ("x", true) if !shift => Clipboard(KeyAction::Cut),
        ("v", true) if !shift => Clipboard(KeyAction::Paste),
        (_, true) => Pass,
        ("enter", _) if shift && r > 0 => Select(r - 1, c),
        ("enter", _) if shift => Stay,
        ("enter", _) => down(),
        ("tab", _) => tab(shift),
        ("left", _) if c > 0 => Select(r, c - 1),
        ("right", _) if c + 1 < t.cols => Select(r, c + 1),
        ("left" | "right", _) => Stay,
        ("up", _) => up_or_leave(),
        ("down", _) => down_or_leave(),
        ("f2", _) => Edit,
        ("backspace" | "delete", _) => Clear,
        ("escape", _) => Escape,
        _ => Pass,
    }
}

/// Where paragraphs `items` go and as what text, with the line the last one ends on: before
/// block `to`, or with `caret` (a line), on that line when it is blank with nothing above it that
/// it would join (the caret's own new paragraph), else before the block after the caret's. Past
/// the last block they go at the end, after a blank line.
fn paragraph_spot(lines: &[String], blocks: &[md_blocks::Block], to: usize, caret: Option<usize>, items: &[String]) -> Option<(edit::Pos, String, usize)> {
    let body = items.join("\n\n");
    let extra = 2 * items.len() - 2;
    let blank = |l: usize| lines.get(l).is_none_or(|t| t.trim().is_empty());
    let to = match caret {
        Some(l) if blank(l) && (l == 0 || blank(l - 1)) => {
            // A free blank line: the paragraphs go on it, a blank line after them when text follows.
            // The line's own break ends the paragraph; one more leaves a blank line before the text.
            let after = if lines.get(l + 1).is_some_and(|t| !t.trim().is_empty()) { "\n" } else { "" };
            return Some((edit::Pos { line: l, ch: 0 }, format!("{body}{after}"), l + extra));
        }
        Some(l) => md_blocks::block_at(blocks, l) + 1,
        None => to,
    };
    match blocks.get(to) {
        Some(b) => Some((edit::Pos { line: b.lines.start, ch: 0 }, format!("{body}\n\n"), b.lines.start + extra)),
        None => {
            let last = lines.len().checked_sub(1)?;
            let end = edit::Pos { line: last, ch: lines[last].chars().count() };
            let lead = match (blank(last), last > 0 && !blank(last.saturating_sub(1))) {
                (false, _) => "\n\n",
                (true, true) => "\n",
                (true, false) => "",
            };
            Some((end, format!("{lead}{body}"), last + lead.matches('\n').count() + extra))
        }
    }
}

/// The cells a Delete clears: the pick's (a row, a column, all), or the caret's cell alone.
fn picked_cells(pick: Option<Pick>, r: usize, c: usize, rows: usize, cols: usize) -> Vec<(usize, usize)> {
    match pick {
        Some(Pick::Row(pr)) => (0..cols).map(|cc| (pr, cc)).collect(),
        Some(Pick::Column(pc)) => (0..rows).map(|rr| (rr, pc)).collect(),
        Some(Pick::All) => (0..rows).flat_map(|rr| (0..cols).map(move |cc| (rr, cc))).collect(),
        None => vec![(r, c)],
    }
}


/// The lines of table block `block` that hold the table (`md_table::line_count`: down to the line
/// before its first blank one), the lines `realign` rewrites. The reference definitions the block
/// holds after them are never in it.
fn table_range(lines: &[String], block: &md_blocks::Block) -> Range<usize> {
    let count = md_table::line_count(&md_view::block_source(lines, block));
    block.lines.start..(block.lines.start + count).min(lines.len())
}

/// The leave check after a caret move, pure: `entry` is the snapshot (`table_entry`: a table's
/// start line and its `table_text`), `blocks` and `lines` the document now. Returns the table to
/// realign, its lines and the snapshot's text, when the caret and its selection have left it and
/// its text differs from the snapshot; and the next snapshot: while the caret or its selection is
/// still in the table, the same one, or with `refresh` (an undo or a redo) the text now of the
/// table the caret is in; otherwise the table the caret is in, as it is now, or `None`.
///
/// `left` (the caret is not placed, after Escape in the rendered view, where the grid draws
/// again) puts the caret in no table: the snapshot's table is left, and no new snapshot is taken.
///
/// The snapshot knows its table by the start line. When the block on that line no longer is a
/// table starting there, the snapshot is dropped and nothing is realigned: an edit that moves
/// the table's start while the caret is inside it (Enter before the header) forgets the
/// snapshot, and the table is then taken as entered with that edit, never realigned for it.
fn next_entry(blocks: &[md_blocks::Block], lines: &[String], caret: edit::Caret, entry: Option<(usize, String)>, refresh: bool, left: bool) -> (Option<(Range<usize>, String)>, Option<(usize, String)>) {
    let block_of = |line: usize| blocks.get(md_blocks::block_at(blocks, line));
    // The block holding line `line` of the caret or its selection: none for an unplaced caret.
    let caret_block = |line: usize| block_of(line).filter(|_| !left);
    let table_at = |start: usize| block_of(start).filter(|b| b.lines.start == start && b.kind == md_blocks::BlockKind::Table);
    let snapshot = |b: &md_blocks::Block| (b.lines.start, table_text(lines, table_range(lines, b)));
    let caret_table = || caret_block(caret.head.line).filter(|b| b.kind == md_blocks::BlockKind::Table).map(snapshot);
    let Some((start, text)) = entry else { return (None, caret_table()) };
    if [caret.head.line, caret.anchor.line].iter().any(|&l| caret_block(l).is_some_and(|b| b.lines.start == start)) {
        let next = if refresh { caret_table().or_else(|| table_at(start).map(snapshot)) } else { Some((start, text)) };
        return (None, next);
    }
    let realign = table_at(start).map(|b| table_range(lines, b)).filter(|r| table_text(lines, r.clone()) != text).map(|r| (r, text));
    (realign, caret_table())
}

/// Lines `range`, each followed by a newline: a table's text as `table_entry` keeps it.
fn table_text(lines: &[String], range: Range<usize>) -> String {
    lines[range].iter().map(|l| format!("{l}\n")).collect()
}

/// The caret left a table whose lines are `lines` (`table_range`): when their
/// text (`table_text`) differs from `entry_text`, the text they had when the caret entered, and
/// `md_table::realign` makes something different of it, they are replaced with the realigned
/// text as one undo step. A table that no longer parses is left as typed. `true` when it edited.
fn realign_on_leave(doc: &mut edit::Document, lines: Range<usize>, entry_text: &str, now: Instant) -> bool {
    if lines.is_empty() || lines.end > doc.lines().len() {
        return false;
    }
    let text = table_text(doc.lines(), lines.clone());
    if text == entry_text {
        return false;
    }
    let Some(aligned) = md_table::realign(&text).filter(|a| *a != text) else { return false };
    let last = lines.end - 1;
    let end = edit::Pos { line: last, ch: doc.lines()[last].chars().count() };
    doc.break_coalescing();
    doc.replace_range(edit::Pos { line: lines.start, ch: 0 }, end, aligned.strip_suffix('\n').unwrap_or(&aligned), now);
    true
}

/// Tab and Shift+Tab in a table block: the caret goes to the start of the next or the previous
/// cell's text (`md_table::next_cell`, `prev_cell`), its selection cleared. On the delimiter row
/// Tab goes to the first body row's first cell and Shift+Tab to the header's last; Shift+Tab from
/// the header's first cell stays; Tab from the last cell of the last row (or the delimiter row of
/// a table with no body) appends a row of empty cells after the table's last line (`table_range`,
/// never after the reference definitions the block holds), one undo step, and lands in its first
/// cell. Spaces are never inserted. `None` when the caret is not in a table's rows (another block,
/// a table that does not parse, the blank lines and the definitions after it).
fn table_tab(doc: &mut edit::Document, caret: &mut edit::Caret, blocks: &[md_blocks::Block], forward: bool, now: Instant) -> Option<Outcome> {
    use md_table::CellPos;
    let head = doc.clamp(caret.head);
    let block = blocks.get(md_blocks::block_at(blocks, head.line)).filter(|b| b.kind == md_blocks::BlockKind::Table)?;
    let source = md_view::block_source(doc.lines(), block);
    md_table::parse(&source)?;
    let last = table_range(doc.lines(), block).end.checked_sub(1)?;
    let end = edit::Pos { line: last, ch: doc.lines()[last].chars().count() };
    let row = head.line.checked_sub(block.lines.start)?;
    let to = match (md_table::cell_at(&source, row, head.ch), forward) {
        (Some(pos), true) => md_table::next_cell(&source, pos),
        (Some(pos), false) => md_table::prev_cell(&source, pos),
        (None, true) if row == 1 => md_table::cell_below(&source, CellPos { row: 1, cell: 0 }),
        (None, false) if row == 1 => md_table::prev_cell(&source, CellPos { row: 1, cell: 0 }),
        (None, _) => return None,
    };
    doc.break_coalescing();
    let at = match to {
        Some(pos) => {
            let (line, ch) = md_table::cell_start(&source, pos)?;
            edit::Pos { line: block.lines.start + line, ch }
        }
        None if !forward => return Some(Outcome::Nothing),
        None => {
            doc.replace_range(end, end, &md_table::appended_row(&source), now);
            let line = end.line + 1;
            let (_, ch) = md_table::cell_start(&doc.lines()[line], CellPos { row: 0, cell: 0 })?;
            *caret = edit::Caret::at(edit::Pos { line, ch });
            return Some(Outcome::Edited);
        }
    };
    *caret = edit::Caret::at(at);
    Some(Outcome::Moved)
}

/// What an editing key asks of a document viewer, before it meets the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// A caret move; `true` extends the selection.
    Move(edit::Move, bool),
    DeleteBack(edit::Unit),
    DeleteFwd(edit::Unit),
    Newline,
    /// Shift+Enter: a hard line break, or out of a quote or a code block.
    BreakLine,
    Tab,
    ShiftTab,
    Undo,
    Redo,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Save,
    Escape,
    /// Home and End scroll the view; the caret stays where it is, as in every Mac text view.
    ScrollHome,
    ScrollEnd,
}

/// The Mac editing keys of a document viewer. Text, a space included, comes through the input
/// handler instead, and a chord this table does not know is left to the keymap. Page Up and Page
/// Down carry no row count; `apply_action` takes it from its caller.
pub fn key_action(ks: &Keystroke) -> Option<KeyAction> {
    use edit::{Move as M, Unit as U};
    use KeyAction as K;
    let m = &ks.modifiers;
    if m.control {
        return None;
    }
    let (cmd, alt, shift) = (m.platform, m.alt, m.shift);
    let mv = |to: M| Some(K::Move(to, shift));
    match (ks.key.as_str(), cmd, alt) {
        ("left", false, false) => mv(M::Left),
        ("right", false, false) => mv(M::Right),
        ("up", false, false) => mv(M::Up),
        ("down", false, false) => mv(M::Down),
        ("left", false, true) => mv(M::WordLeft),
        ("right", false, true) => mv(M::WordRight),
        ("left", true, false) => mv(M::LineStart),
        ("right", true, false) => mv(M::LineEnd),
        ("up", true, false) => mv(M::DocStart),
        ("down", true, false) => mv(M::DocEnd),
        ("pageup", false, false) => mv(M::PageUp(0)),
        ("pagedown", false, false) => mv(M::PageDown(0)),
        ("home", false, false) if !shift => Some(K::ScrollHome),
        ("end", false, false) if !shift => Some(K::ScrollEnd),
        ("backspace", false, false) => Some(K::DeleteBack(U::Grapheme)),
        ("backspace", false, true) if !shift => Some(K::DeleteBack(U::Word)),
        ("backspace", true, false) if !shift => Some(K::DeleteBack(U::LineStart)),
        ("delete", false, false) => Some(K::DeleteFwd(U::Grapheme)),
        ("delete", false, true) if !shift => Some(K::DeleteFwd(U::Word)),
        ("delete", true, false) if !shift => Some(K::DeleteFwd(U::LineEnd)),
        ("enter", false, false) => Some(if shift { K::BreakLine } else { K::Newline }),
        ("tab", false, false) => Some(if shift { K::ShiftTab } else { K::Tab }),
        ("escape", false, false) if !shift => Some(K::Escape),
        ("z", true, false) => Some(if shift { K::Redo } else { K::Undo }),
        ("a", true, false) if !shift => Some(K::SelectAll),
        ("c", true, false) if !shift => Some(K::Copy),
        ("x", true, false) if !shift => Some(K::Cut),
        ("v", true, false) if !shift => Some(K::Paste),
        ("s", true, false) if !shift => Some(K::Save),
        _ => None,
    }
}

/// What a key does in the rendered view before the user has placed the caret, so nothing acts on
/// a caret they cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrePlacement {
    /// The view scrolls, as the read-only viewer's did; the caret stays unplaced.
    Scroll,
    /// The caret goes to the start of the first block on screen, then the key applies there.
    Place,
    /// Nothing happens and the caret stays unplaced.
    Ignore,
}

/// The decision table behind `PrePlacement`: Page Up and Down, Home, End, Up and Down scroll; an
/// edit (and Undo, Redo, Select All) places the caret first; Escape, Copy and the sideways moves
/// do nothing. Cmd+S saves whatever this says; the caller runs it before asking.
pub fn pre_placement_action(action: KeyAction) -> PrePlacement {
    use edit::Move as M;
    use KeyAction as K;
    match action {
        K::Move(M::PageUp(_) | M::PageDown(_) | M::Up | M::Down, _) | K::ScrollHome | K::ScrollEnd => PrePlacement::Scroll,
        K::DeleteBack(_) | K::DeleteFwd(_) | K::Newline | K::BreakLine | K::Tab | K::ShiftTab | K::Undo | K::Redo | K::Cut | K::Paste | K::SelectAll => PrePlacement::Place,
        K::Move(..) | K::Copy | K::Escape | K::Save => PrePlacement::Ignore,
    }
}

/// Whether a key the document took puts the caret in play (a move or an edit), so the rendered
/// view shows the caret's block raw from then on. Escape, Copy, Save and the scroll keys do not.
fn places_caret(action: KeyAction) -> bool {
    !matches!(action, KeyAction::Escape | KeyAction::Copy | KeyAction::Save | KeyAction::ScrollHome | KeyAction::ScrollEnd)
}

/// What `apply_action` did, for the caller to finish: the clipboard, a save, the find bar, the
/// scroll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Edited,
    Moved,
    /// The selected text, for the clipboard.
    Copy(String),
    /// The text cut out of the document, for the clipboard.
    Cut(String),
    /// The caller inserts `normalize_paste` of the clipboard's text.
    Paste,
    Save,
    Escape,
    Scroll(KeyAction),
    Nothing,
}

/// Pasted text with the document's line breaks: `\r\n` and a lone `\r` become `\n`.
pub fn normalize_paste(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The fence a line opens: three or more backticks or tildes at the line start, then an optional
/// info string (which, after backticks, holds no backtick).
fn fence_opener(line: &str) -> Option<&str> {
    let c = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = line.chars().take_while(|&x| x == c).count();
    if n < 3 || (c == '`' && line[n..].contains('`')) {
        return None;
    }
    Some(&line[..n])
}

/// Whether `line` closes `fence`: the same character at least as many times, up to three spaces
/// of indent, nothing else but trailing spaces.
fn closes_fence(line: &str, fence: &str) -> bool {
    let body = line.trim_end();
    let trimmed = body.trim_start_matches(' ');
    let c = fence.chars().next().unwrap_or('`');
    body.len() - trimmed.len() <= 3 && trimmed.len() >= fence.len() && trimmed.chars().all(|x| x == c)
}

/// Whether the code block `b` is closed: its last non-blank line after its first closes `fence`.
fn block_closed(lines: &[String], b: &md_blocks::Block, fence: &str) -> bool {
    let end = b.lines.end.min(lines.len());
    (b.lines.start + 1..end).rev().find(|&i| !lines[i].trim().is_empty()).is_some_and(|last| closes_fence(&lines[last], fence))
}

/// Whether a code block is closed. An indented block has no fence to close and counts as closed.
fn code_block_closed(lines: &[String], b: &md_blocks::Block) -> bool {
    match lines.get(b.lines.start).and_then(|l| fence_opener(l.trim_start())) {
        Some(opener) => block_closed(lines, b, opener),
        None => true,
    }
}

/// Whether Enter closes the fence on `line`: only when the fences from that line on are
/// unbalanced, that is when the last code block at or after the line runs to the end of the
/// document unclosed. A fence typed into a balanced document makes the count odd; an existing
/// opener leaves it even. A line inside a code block that starts above it (a closer, or code) never
/// gets a closer. A structure taken before the fence was typed (no code block on the line) asks
/// the same question of the blocks below, which the new fence would leave unbalanced when they
/// are balanced now.
fn fence_needs_closing(lines: &[String], blocks: &[md_blocks::Block], line: usize) -> bool {
    let is_code = |b: &&md_blocks::Block| matches!(b.kind, md_blocks::BlockKind::CodeBlock { .. });
    let last_from_line = || blocks.iter().filter(is_code).filter(|b| b.lines.start >= line).last();
    match blocks.get(md_blocks::block_at(blocks, line)).filter(|b| is_code(b) && b.lines.contains(&line)) {
        Some(b) if b.lines.start != line => false,
        Some(_) => last_from_line().is_some_and(|last| !code_block_closed(lines, last)),
        None => last_from_line().is_none_or(|last| code_block_closed(lines, last)),
    }
}

/// Enter: a new fence closes itself, a list item continues (an empty one ends the list), anything
/// else is a plain line break.
fn newline(doc: &mut edit::Document, caret: edit::Caret, blocks_before: &[md_blocks::Block], now: Instant) -> edit::Caret {
    if caret.head == caret.anchor {
        let at = doc.clamp(caret.head);
        let line = doc.lines()[at.line].clone();
        let len = line.chars().count();
        if at.ch == len {
            if let Some(fence) = fence_opener(&line).filter(|_| fence_needs_closing(doc.lines(), blocks_before, at.line)) {
                doc.insert(caret, &format!("\n\n{fence}"), now);
                return edit::Caret::at(edit::Pos { line: at.line + 1, ch: 0 });
            }
        }
        // Enter in the indent before a marker is a plain break: the item moves down whole.
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        if at.ch > indent {
            if let Some((marker, empty)) = md_blocks::list_continuation(&line) {
                // An empty nested item moves out a level; an empty top-level one ends the list.
                if empty && indent > 0 {
                    let mut c = edit::Caret::at(at);
                    if indent_item(doc, &mut c, false, now) == Some(Outcome::Edited) {
                        return edit::Caret::at(edit::Pos { line: c.head.line, ch: doc.lines()[c.head.line].chars().count() });
                    }
                }
                if empty {
                    let start = doc.replace_range(edit::Pos { line: at.line, ch: 0 }, edit::Pos { line: at.line, ch: len }, "", now);
                    return edit::Caret::at(start);
                }
                return doc.insert(caret, &format!("\n{marker}"), now);
            }
        }
    }
    doc.insert(caret, "\n", now)
}

/// Tab (`deeper`) or Shift+Tab with the caret anywhere in a list item: the item, with its
/// continuation lines and the items nested in it, moves to the content column of the nearest item
/// above it at its level or shallower (one level deeper at most, as markdown nests), or back to
/// the column of the item it is nested in. One undoable edit. `Some(Nothing)` in an item that
/// cannot move (the list's first item, an item already one level down, a top-level item on
/// Shift+Tab), so Tab does not type spaces there; `None` outside a list.
fn indent_item(doc: &mut edit::Document, caret: &mut edit::Caret, deeper: bool, now: Instant) -> Option<Outcome> {
    let lines = doc.lines();
    let width = |l: &str| l.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
    let item = |n: usize| md_blocks::list_item_columns(&lines[n]);
    // The item holding the caret: its own line, or the item line above a continuation line.
    let here = doc.clamp(caret.head).line;
    let mut at = here;
    while item(at).is_none() {
        let l = &lines[at];
        if at == 0 || l.trim().is_empty() || width(l) == 0 {
            return None;
        }
        at -= 1;
    }
    let (indent, _) = item(at)?;
    // Its lines: the item line, then every line below indented past its marker.
    let mut end = at;
    while end + 1 < lines.len() && !lines[end + 1].trim().is_empty() && width(&lines[end + 1]) > indent {
        end += 1;
    }
    // The items above it, nearest first, skipping their continuation lines.
    let above = (0..at).rev().take_while(|&n| !lines[n].trim().is_empty()).filter_map(item);
    let target = if deeper {
        match above.into_iter().find(|&(i, _)| i <= indent) {
            Some((_, content)) if content > indent => content,
            _ => return Some(Outcome::Nothing),
        }
    } else if indent == 0 {
        return Some(Outcome::Nothing);
    } else {
        above.into_iter().find(|&(i, _)| i < indent).map_or(0, |(i, _)| i)
    };
    let shift = target as isize - indent as isize;
    // A numbered item takes the number after the sibling it lands beside, or 1 when it starts a
    // sublist (markdown starts a nested numbered list only at 1).
    let number = {
        let sibling = (0..at).rev().take_while(|&n| !lines[n].trim().is_empty()).filter_map(|n| item(n).map(|c| (n, c))).find(|&(_, (i, _))| i <= target);
        match sibling {
            Some((n, (i, _))) if i == target => md_blocks::list_number(&lines[n]).map(|v| v + 1),
            _ if deeper => Some(1),
            _ => None,
        }
    };
    let mut lines_moved: Vec<String> = lines[at..=end].to_vec();
    if let Some(renumbered) = number.and_then(|v| md_blocks::renumber(&lines_moved[0], v)) {
        lines_moved[0] = renumbered;
    }
    let moved: Vec<String> = lines_moved
        .iter()
        .map(|l| {
            if shift > 0 {
                format!("{}{l}", " ".repeat(shift as usize))
            } else {
                let lead = l.chars().take_while(|c| *c == ' ').count();
                l.chars().skip(lead.min((-shift) as usize)).collect()
            }
        })
        .collect();
    let deltas: Vec<isize> = lines[at..=end].iter().zip(&moved).map(|(a, b)| b.chars().count() as isize - a.chars().count() as isize).collect();
    let last = lines[end].chars().count();
    doc.replace_range(edit::Pos { line: at, ch: 0 }, edit::Pos { line: end, ch: last }, &moved.join("\n"), now);
    for p in [&mut caret.head, &mut caret.anchor] {
        if (at..=end).contains(&p.line) {
            p.ch = (p.ch as isize + deltas[p.line - at]).max(0) as usize;
        }
    }
    caret.goal_col = None;
    Some(Outcome::Edited)
}

/// Applies an action to the document and caret; `blocks_before` is the block structure before
/// this action (the fence rule and a table's Tab and Shift+Tab read it; empty, they do not apply).
/// Pure: no gpui.
pub fn apply_action(doc: &mut edit::Document, caret: &mut edit::Caret, blocks_before: &[md_blocks::Block], action: KeyAction, rows: usize, now: Instant) -> Outcome {
    use edit::Move as M;
    use KeyAction as K;
    let selected = |doc: &edit::Document, c: &edit::Caret| doc.text_in(c.anchor, c.head);
    match action {
        K::Move(to, select) => {
            let to = match to {
                M::PageUp(_) => M::PageUp(rows.max(1)),
                M::PageDown(_) => M::PageDown(rows.max(1)),
                other => other,
            };
            doc.break_coalescing();
            *caret = doc.move_by(*caret, to, select);
            Outcome::Moved
        }
        K::SelectAll => {
            doc.break_coalescing();
            *caret = doc.select_all();
            Outcome::Moved
        }
        K::DeleteBack(unit) => {
            *caret = doc.delete_backward(*caret, unit, now);
            Outcome::Edited
        }
        K::DeleteFwd(unit) => {
            *caret = doc.delete_forward(*caret, unit, now);
            Outcome::Edited
        }
        K::Newline => {
            *caret = newline(doc, *caret, blocks_before, now);
            Outcome::Edited
        }
        K::BreakLine => {
            *caret = doc.insert(*caret, "\n", now);
            Outcome::Edited
        }
        // A table's cells come before a list item's indent.
        K::Tab => table_tab(doc, caret, blocks_before, true, now).or_else(|| indent_item(doc, caret, true, now)).unwrap_or_else(|| {
            *caret = doc.insert(*caret, "    ", now);
            Outcome::Edited
        }),
        K::ShiftTab => table_tab(doc, caret, blocks_before, false, now).or_else(|| indent_item(doc, caret, false, now)).unwrap_or(Outcome::Nothing),
        K::Undo | K::Redo => match if action == K::Undo { doc.undo() } else { doc.redo() } {
            Some(c) => {
                *caret = c;
                Outcome::Edited
            }
            None => Outcome::Nothing,
        },
        K::Copy => Some(selected(doc, caret)).filter(|t| !t.is_empty()).map_or(Outcome::Nothing, Outcome::Copy),
        K::Cut => {
            let text = selected(doc, caret);
            if text.is_empty() {
                return Outcome::Nothing;
            }
            *caret = doc.delete_backward(*caret, edit::Unit::Grapheme, now);
            Outcome::Cut(text)
        }
        K::Paste => Outcome::Paste,
        K::Save => Outcome::Save,
        K::Escape => Outcome::Escape,
        K::ScrollHome | K::ScrollEnd => Outcome::Scroll(action),
    }
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
        let (mode, presenting) = match self.tree.locate(tab).and_then(|(pane, idx)| self.tree.pane(pane)?.tabs.get(idx)) {
            Some(Tab::Viewer { mode, presenting, .. }) => (*mode, *presenting && *mode == ViewMode::Rendered),
            _ => (ViewMode::default(), false),
        };
        let open_details = self.details_open.get(path).map(|saved| sections_from(saved)).unwrap_or_default();
        let anchor = self.pending_anchors.remove(path);
        let diagram_modes: std::collections::BTreeMap<usize, workspace::DiagramMode> = self.diagram_modes.get(path).map(|d| d.iter().map(|d| (d.index, d.mode)).collect()).unwrap_or_default();
        let image_destination = self.config.viewer.image_destination.clone();
        let folded: std::collections::BTreeSet<md_fold::FoldKey> = self.folds.get(path).map(|f| f.iter().map(|f| (f.heading.clone(), f.index)).collect()).unwrap_or_default();
        let v = cx.new(|cx| {
            let mut v = Viewer::new(path.to_path_buf(), font_size, line_height, cx);
            v.pending_anchor = anchor;
            v.diagram_modes = diagram_modes;
            v.image_destination = image_destination;
            v.mode = mode;
            // Presentation mode comes back with the tab.
            v.presenting = presenting;
            v.open_details = open_details;
            v.folded = folded;
            v
        });
        cx.subscribe(&v, move |root, v, event: &ViewerEvent, cx| {
            match event {
                // An edited preview is kept: the next file opens in a tab of its own.
                ViewerEvent::Edited if root.preview_tab == Some(tab) => root.preview_tab = None,
                ViewerEvent::Edited => {}
                // A watcher pass ignored while the save ran is looked at again however it ended.
                ViewerEvent::Saved | ViewerEvent::SaveFailed => {
                    if v.update(cx, |v, _| std::mem::take(&mut v.recheck_after_save)) {
                        root.load(tab, true, cx);
                    }
                }
                ViewerEvent::OpenPath(path, anchor) => {
                    let _via = crate::telemetry::opened_via(crate::telemetry::Opened::Link);
                    root.open_viewer(path, true, cx);
                    // The file's viewer goes to the heading now, or once it has loaded; a viewer
                    // not made yet takes it when the pane first shows the tab.
                    if let Some(anchor) = anchor.clone() {
                        let open = root.viewers.values().find(|o| o.read(cx).path == *path).cloned();
                        match open {
                            Some(o) => o.update(cx, |o, cx| o.anchor_to(anchor, cx)),
                            None => {
                                root.pending_anchors.insert(path.clone(), anchor);
                            }
                        }
                    }
                }
                ViewerEvent::OpenVideo(path) => root.open_poster_video(tab, path, cx),
                ViewerEvent::OpenPage(path) => root.open_beside_document(tab, path, crate::telemetry::Opened::Link, cx),
                // The state is the file's: every other tab on it shows the same sections open.
                ViewerEvent::DetailsToggled(open) => {
                    let path = v.read(cx).path.clone();
                    workspace::set_details_open(&mut root.details_open, &path, open.clone());
                    let shown = sections_from(open);
                    for other in root.viewers.values().filter(|o| o.entity_id() != v.entity_id()) {
                        other.update(cx, |o, cx| {
                            if o.path == path {
                                o.open_details = shown.clone();
                                cx.notify();
                            }
                        });
                    }
                    root.save(cx);
                }
                // So are the diagrams' modes.
                ViewerEvent::DiagramModesChanged(views) => {
                    let path = v.read(cx).path.clone();
                    workspace::set_diagram_modes(&mut root.diagram_modes, &path, views.clone());
                    let modes: std::collections::BTreeMap<usize, workspace::DiagramMode> = views.iter().map(|d| (d.index, d.mode)).collect();
                    for other in root.viewers.values().filter(|o| o.entity_id() != v.entity_id()) {
                        other.update(cx, |o, cx| {
                            if o.path == path {
                                o.diagram_modes = modes.clone();
                                cx.notify();
                            }
                        });
                    }
                    root.save(cx);
                }
                // The folds are the file's too.
                ViewerEvent::FoldsChanged(folds) => {
                    let path = v.read(cx).path.clone();
                    workspace::set_folds(&mut root.folds, &path, folds.clone());
                    let keys: std::collections::BTreeSet<md_fold::FoldKey> = folds.iter().map(|f| (f.heading.clone(), f.index)).collect();
                    for other in root.viewers.values().filter(|o| o.entity_id() != v.entity_id()) {
                        other.update(cx, |o, cx| {
                            if o.path == path {
                                o.folded = keys.clone();
                                o.refresh_structure();
                                cx.notify();
                            }
                        });
                    }
                    root.save(cx);
                }
            }
            // The tab's label shows whether it is dirty.
            cx.notify();
        })
        .detach();
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

    /// Reloads the viewers whose colours depend on the highlighter: plain text, binaries, files
    /// too large to show and markdown documents (a theme change repaints them) have nothing to
    /// recolour.
    pub(crate) fn reload_all_viewers(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<TabId> = self.viewers.iter().filter(|(_, v)| {
            let v = v.read(cx);
            v.plain.is_none() && v.doc.is_none() && matches!(v.content, viewer::Content::Text { .. })
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
            let (path, shown, text, applied) = {
                let v = viewer.read(cx);
                match (&v.content, &v.doc) {
                    // Unsaved edits have no diff until they are saved.
                    (_, Some(_)) if v.dirty => continue,
                    // The text is joined on the background executor, never here.
                    (Content::Text { lines, .. }, Some(doc)) if v.loaded => (v.path.clone(), lines.clone(), Some((doc.ending, doc.bom, doc.trailing_newline)), v.applied),
                    (Content::Text { lines, .. }, None) if v.loaded && v.plain != Some(Plain::TooBig) => (v.path.clone(), lines.clone(), None, v.applied),
                    _ => continue,
                }
            };
            let weak = viewer.downgrade();
            cx.spawn(async move |_, cx| {
                let lines = shown.clone();
                let diff = cx
                    .background_executor()
                    .spawn(async move {
                        match text {
                            // A document's lines keep their tabs; the base's are split for display.
                            Some((ending, bom, trailing)) => diff_of(&path, &split_lines(&edit::join_text(&lines, ending, bom, trailing))),
                            None => diff_of(&path, &lines),
                        }
                    })
                    .await;
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
                if !should_apply(seq, v.applied) {
                    return;
                }
                // A document decides here, with the state it has now, never before the read.
                if v.doc.is_some() {
                    match disk_change(loaded.fingerprint, v.fingerprint, v.dirty, v.saving) {
                        DiskChange::Ignore => {
                            if v.saving && loaded.fingerprint != v.fingerprint {
                                v.recheck_after_save = true;
                            }
                            return;
                        }
                        DiskChange::Conflict => {
                            v.conflict = true;
                            cx.notify();
                            return;
                        }
                        DiskChange::Reload => {}
                    }
                }
                v.applied = seq;
                v.apply(loaded, cx);
            });
        })
        .detach();
    }

    /// The focused pane's active tab, when it is a viewer whose viewer exists.
    pub fn active_viewer_tab(&self) -> Option<TabId> {
        let p = self.tree.pane(self.tree.focused)?;
        let id = *p.ids.get(p.active)?;
        (matches!(p.tabs.get(p.active)?, Tab::Viewer { .. }) && self.viewers.contains_key(&id)).then_some(id)
    }

    /// Writes every viewer's unsaved edits now, on this thread, for a quit; returns the errors
    /// (conflicts and failed writes) with their tabs, whose edits are not on disk.
    pub fn save_dirty_viewers_now(&mut self, cx: &mut App) -> Vec<(TabId, String)> {
        let viewers: Vec<(TabId, Entity<Viewer>)> = self.viewers.iter().map(|(t, v)| (*t, v.clone())).collect();
        viewers
            .into_iter()
            .filter_map(|(tab, v)| {
                v.update(cx, |v, cx| {
                    let saved = v.save_now();
                    if saved.is_err() {
                        cx.notify();
                    }
                    // The conflict's text does not name the file; a write error's does.
                    saved.err().map(|e| (tab, if e == CONFLICT_NOTICE { format!("{}: {e}", v.path.display()) } else { e }))
                })
            })
            .collect()
    }

    /// Writes every viewer's edits that are still not on disk to the recovery folder, for a quit
    /// or a relaunch that goes ahead without them, and says where on stderr. Returns the files
    /// written.
    pub fn recover_unsaved(&mut self, cx: &mut App) -> Vec<PathBuf> {
        let dir = crate::paths::recovered_dir();
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let viewers: Vec<Entity<Viewer>> = self.viewers.values().cloned().collect();
        let mut written = Vec::new();
        for v in viewers {
            let path = v.read(cx).path.clone();
            match v.update(cx, |v, _| v.recover(&dir, unix)) {
                Some(Ok(to)) => {
                    eprintln!("arkos: unsaved edits of {} written to {}", path.display(), to.display());
                    written.push(to);
                }
                Some(Err(e)) => eprintln!("arkos: the unsaved edits of {} are lost: cannot write them to {}: {e}", path.display(), dir.display()),
                None => {}
            }
        }
        written
    }

    /// Flips a markdown tab between rendered and raw, in the layout (saved) and in its viewer. Raw
    /// ends presentation mode (only the rendered view presents).
    pub fn toggle_markdown_view_for(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some((pane, idx)) = self.tree.locate(tab) else { return };
        let Some(Tab::Viewer { path, mode, .. }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() else { return };
        if !is_markdown(&path) {
            return;
        }
        let next = match mode {
            ViewMode::Rendered => ViewMode::Raw,
            ViewMode::Raw => ViewMode::Rendered,
        };
        self.tree.set_viewer_mode(pane, idx, next);
        if next == ViewMode::Raw {
            self.tree.set_viewer_presenting(pane, idx, false);
        }
        if let Some(v) = self.viewers.get(&tab).cloned() {
            v.update(cx, |v, cx| v.set_mode(next, cx));
        }
        self.save(cx);
        cx.notify();
    }

    /// Presentation mode on or off in a markdown tab, in the layout (saved with the tab) and in its
    /// viewer: from Raw it shows Rendered too, since only the rendered view presents.
    pub fn toggle_presentation_for(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some((pane, idx)) = self.tree.locate(tab) else { return };
        let Some(Tab::Viewer { path, mode, presenting }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() else { return };
        if !is_markdown(&path) {
            return;
        }
        if mode == ViewMode::Raw {
            self.toggle_markdown_view_for(tab, cx);
        }
        let on = !presenting || mode == ViewMode::Raw;
        self.tree.set_viewer_presenting(pane, idx, on);
        if let Some(v) = self.viewers.get(&tab).cloned() {
            v.update(cx, |v, cx| v.set_presenting(on, cx));
        }
        self.save(cx);
        cx.notify();
    }

    /// The focused pane's active viewer, once its tab has been shown.
    pub fn active_viewer(&self) -> Option<Entity<Viewer>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Viewer { .. } => self.viewers.get(p.ids.get(p.active)?).cloned(),
            Tab::Terminal { .. } | Tab::Browser { .. } | Tab::Video { .. } => None,
        }
    }
}

/// Whether a `<details>` section is shown open: the state `states` holds for it (by its summary
/// and its index among the file's sections) when there is one, else `starts_open`, its tag's
/// `open` attribute.
pub fn section_shown_open(states: &BTreeMap<(String, usize), bool>, summary: &str, index: usize, starts_open: bool) -> bool {
    states.iter().find(|((s, i), _)| *i == index && s == summary).map_or(starts_open, |(_, open)| *open)
}

/// Block `ix`'s index among the `<details>` sections in `blocks`, in document order: the sections
/// before it. A section's state is kept by its summary and this index, never by its line, so a
/// paragraph added above it keeps the state.
pub fn section_index(blocks: &[md_blocks::Block], ix: usize) -> usize {
    blocks[..ix.min(blocks.len())].iter().filter(|b| b.kind == md_blocks::BlockKind::Details).count()
}

/// Opens a shown-closed section or closes a shown-open one, recording the new state in `states`.
pub fn flip_section(states: &mut BTreeMap<(String, usize), bool>, summary: &str, index: usize, starts_open: bool) {
    let open = section_shown_open(states, summary, index, starts_open);
    states.insert((summary.to_string(), index), !open);
}

/// A viewer's section states as the workspace file keeps them.
pub fn sections_saved(states: &BTreeMap<(String, usize), bool>) -> Vec<DetailsOpen> {
    states.iter().map(|((summary, index), open)| DetailsOpen { summary: summary.clone(), index: *index, open: *open }).collect()
}

/// The section states the workspace file keeps for a file, as a viewer holds them.
pub fn sections_from(saved: &[DetailsOpen]) -> BTreeMap<(String, usize), bool> {
    saved.iter().map(|d| ((d.summary.clone(), d.index), d.open)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Move, Pos, Unit};

    #[test]
    fn a_section_s_saved_state_wins_over_its_open_attribute_across_a_save() {
        let mut states = BTreeMap::new();
        flip_section(&mut states, "Open", 0, true);
        flip_section(&mut states, "Closed", 1, false);
        let mut ws = workspace::Workspace::default();
        workspace::set_details_open(&mut ws.details_open, Path::new("/a.md"), sections_saved(&states));
        let back: workspace::Workspace = serde_json::from_str(&serde_json::to_string(&ws).unwrap()).unwrap();
        let restored = sections_from(&back.details_open[Path::new("/a.md")]);
        assert!(!section_shown_open(&restored, "Open", 0, true), "closed although its tag says open");
        assert!(section_shown_open(&restored, "Closed", 1, false), "opened although its tag does not say so");
        assert!(section_shown_open(&restored, "Other", 2, true) && !section_shown_open(&restored, "Other", 2, false), "no state: the tag decides");
    }

    #[test]
    fn a_section_s_state_survives_a_paragraph_inserted_above_it() {
        let section = "<details>\n<summary>Setup</summary>\n\nSteps.\n\n</details>\n";
        let before = format!("# Title\n\n{section}\n{section}");
        let blocks = md_blocks::split(edit::Document::from_text(&before).lines());
        let sections: Vec<usize> = (0..blocks.len()).filter(|&i| blocks[i].kind == md_blocks::BlockKind::Details).collect();
        assert_eq!(sections.len(), 2, "{blocks:?}");
        // The second "Setup" is opened, and the state saved and read back.
        let mut states = BTreeMap::new();
        flip_section(&mut states, "Setup", section_index(&blocks, sections[1]), false);
        let states = sections_from(&sections_saved(&states));
        // A paragraph (and a list) above both: the sections move down a few lines and blocks.
        let after = format!("# Title\n\nA new paragraph.\n\n- and\n- a list\n\n{section}\n{section}");
        let moved = md_blocks::split(edit::Document::from_text(&after).lines());
        let shifted: Vec<usize> = (0..moved.len()).filter(|&i| moved[i].kind == md_blocks::BlockKind::Details).collect();
        assert_ne!(shifted, sections, "the blocks moved");
        assert!(!section_shown_open(&states, "Setup", section_index(&moved, shifted[0]), false), "the first stays closed");
        assert!(section_shown_open(&states, "Setup", section_index(&moved, shifted[1]), false), "the second stays open");
    }

    #[test]
    fn tab_in_raw_mode_inserts_spaces_in_a_table_and_indents_a_list_item() {
        // Raw mode gives `apply_action` no blocks (ruling A: the table keys are the rendered
        // view's), so Tab in a table is a plain Tab.
        let now = Instant::now();
        let text = "| a | b |\n|---|---|\n| 1 | 2 |\n\n- first\n- item\n";
        let mut d = edit::Document::from_text(text);
        let mut c = edit::Caret::at(Pos { line: 0, ch: 2 });
        assert_eq!(apply_action(&mut d, &mut c, &[], KeyAction::Tab, 20, now), Outcome::Edited);
        assert_eq!(d.lines()[0], "|     a | b |", "four spaces, not the next cell");
        assert_eq!((c.head.line, c.head.ch), (0, 6));
        let mut c = edit::Caret::at(Pos { line: 2, ch: 6 });
        assert_eq!(apply_action(&mut d, &mut c, &[], KeyAction::Tab, 20, now), Outcome::Edited);
        assert_eq!(d.lines()[2], "| 1 |     2 |", "the last cell gets spaces too, and no row is added");
        assert_eq!(d.lines().len(), 7);
        let mut c = edit::Caret::at(Pos { line: 0, ch: 6 });
        assert_eq!(apply_action(&mut d, &mut c, &[], KeyAction::ShiftTab, 20, now), Outcome::Nothing, "Shift+Tab in a table does nothing");
        let mut c = edit::Caret::at(Pos { line: 5, ch: 3 });
        apply_action(&mut d, &mut c, &[], KeyAction::Tab, 20, now);
        assert_eq!(d.lines()[5], "  - item", "a list item still indents");
    }

    #[test]
    fn table_keys_walk_the_cells_grow_at_the_end_and_leave_at_the_edges() {
        let ks = |k: &str| Keystroke::parse(k).unwrap();
        // A 2 by 2 table (header and one row), the caret's cell selected.
        let cell = |r, c| TableCell { r, c, rows: 2, cols: 2, ch: 4, text: 2..5, editing: false };
        assert_eq!(table_move(&ks("tab"), &cell(0, 0)), TableMove::Select(0, 1));
        assert_eq!(table_move(&ks("tab"), &cell(0, 1)), TableMove::Select(1, 0), "over to the next row");
        assert_eq!(table_move(&ks("tab"), &cell(1, 1)), TableMove::AppendRow(0), "Tab from the last cell adds a row");
        assert_eq!(table_move(&ks("shift-tab"), &cell(1, 0)), TableMove::Select(0, 1));
        assert_eq!(table_move(&ks("shift-tab"), &cell(0, 0)), TableMove::Stay);
        assert_eq!(table_move(&ks("enter"), &cell(0, 1)), TableMove::Select(1, 1));
        assert_eq!(table_move(&ks("enter"), &cell(1, 1)), TableMove::AppendRow(1), "Enter from the last row adds one");
        assert_eq!(table_move(&ks("shift-enter"), &cell(1, 1)), TableMove::Select(0, 1));
        assert_eq!(table_move(&ks("up"), &cell(0, 0)), TableMove::Leave(true));
        assert_eq!(table_move(&ks("down"), &cell(1, 0)), TableMove::Leave(false));
        assert_eq!(table_move(&ks("right"), &cell(0, 1)), TableMove::Stay, "the arrows stop at the sides");
        assert_eq!(table_move(&ks("left"), &cell(0, 1)), TableMove::Select(0, 0));
        assert_eq!(table_move(&ks("f2"), &cell(0, 0)), TableMove::Edit);
        assert_eq!(table_move(&ks("backspace"), &cell(0, 0)), TableMove::Clear);
        assert_eq!(table_move(&ks("cmd-c"), &cell(0, 0)), TableMove::Clipboard(KeyAction::Copy));
        assert_eq!(table_move(&ks("cmd-z"), &cell(0, 0)), TableMove::Pass, "undo is the editor's");
        assert_eq!(table_move(&ks("a"), &cell(0, 0)), TableMove::Pass, "typing goes to the editor, which replaces the cell");
    }

    #[test]
    fn in_an_editing_cell_the_arrows_move_in_its_text_and_never_cross_a_pipe() {
        let ks = |k: &str| Keystroke::parse(k).unwrap();
        let cell = |ch| TableCell { r: 1, c: 0, rows: 3, cols: 2, ch, text: 2..5, editing: true };
        assert_eq!(table_move(&ks("left"), &cell(3)), TableMove::Pass, "inside the text the editor moves the caret");
        assert_eq!(table_move(&ks("left"), &cell(2)), TableMove::Stay);
        assert_eq!(table_move(&ks("right"), &cell(5)), TableMove::Stay);
        assert_eq!(table_move(&ks("backspace"), &cell(2)), TableMove::Stay, "a Backspace at the start would reach the pipe");
        assert_eq!(table_move(&ks("backspace"), &cell(3)), TableMove::Pass);
        assert_eq!(table_move(&ks("delete"), &cell(5)), TableMove::Stay);
        assert_eq!(table_move(&ks("cmd-left"), &cell(4)), TableMove::Caret(2));
        assert_eq!(table_move(&ks("end"), &cell(2)), TableMove::Caret(5));
        assert_eq!(table_move(&ks("escape"), &cell(3)), TableMove::Select(1, 0), "Escape goes back to selecting the cell");
        assert_eq!(table_move(&ks("enter"), &cell(3)), TableMove::Select(2, 0));
        assert_eq!(table_move(&ks("alt-left"), &cell(3)), TableMove::Pass, "word moves are the editor's");
        assert_eq!(table_move(&ks("cmd-c"), &cell(3)), TableMove::Pass, "the clipboard works on the text");
    }

    #[test]
    fn pasted_and_dropped_files_go_in_as_paragraphs_of_their_own() {
        let run = |text: &str, to: usize, caret: Option<usize>| {
            let mut d = edit::Document::from_text(text);
            let blocks = md_blocks::split(d.lines());
            let (at, ins, last) = paragraph_spot(d.lines(), &blocks, to, caret, &["![a](a.png)".to_string(), "![b](b.png)".to_string()]).unwrap();
            d.replace_range(at, at, &ins, Instant::now());
            (d.lines().join("\n"), d.lines()[last].clone())
        };
        let doc = "one\n\ntwo\n";
        assert_eq!(run(doc, 1, None).0, "one\n\n![a](a.png)\n\n![b](b.png)\n\ntwo\n", "dropped before a block");
        assert_eq!(run(doc, usize::MAX, Some(0)), ("one\n\n![a](a.png)\n\n![b](b.png)\n\ntwo\n".into(), "![b](b.png)".into()), "pasted mid-text: after the caret's block");
        assert_eq!(run(doc, usize::MAX, Some(2)).0, "one\n\ntwo\n\n![a](a.png)\n\n![b](b.png)", "after the last block: at the end");
        assert_eq!(run("one\n\n\ntwo", usize::MAX, Some(2)).0, "one\n\n![a](a.png)\n\n![b](b.png)\n\ntwo", "a free blank line takes them, apart from the text below");
        assert_eq!(run("one", 9, None).0, "one\n\n![a](a.png)\n\n![b](b.png)");
    }

    #[test]
    fn delete_clears_the_picked_row_column_or_grid_or_else_the_cell() {
        assert_eq!(picked_cells(None, 1, 0, 2, 2), vec![(1, 0)]);
        assert_eq!(picked_cells(Some(Pick::Row(1)), 0, 0, 2, 2), vec![(1, 0), (1, 1)]);
        assert_eq!(picked_cells(Some(Pick::Column(1)), 0, 0, 3, 2), vec![(0, 1), (1, 1), (2, 1)]);
        assert_eq!(picked_cells(Some(Pick::All), 0, 0, 2, 2).len(), 4);
    }

    #[test]
    fn tab_walks_the_cells_and_tab_at_the_end_adds_a_row() {
        let text = "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let mut d = edit::Document::from_text(text);
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret::at(edit::Pos { line: 2, ch: 2 }); // In "a".
        let now = Instant::now();
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!((c.head.line, c.head.ch), (2, 6), "the next cell's text");
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!((c.head.line, c.head.ch), (4, 2), "over the delimiter row");
        apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
        assert_eq!((c.head.line, c.head.ch), (2, 6));
        let mut c = edit::Caret::at(edit::Pos { line: 2, ch: 2 });
        apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
        assert_eq!((c.head.line, c.head.ch), (2, 2), "Shift+Tab from the first cell stays");
        let mut c = edit::Caret::at(edit::Pos { line: 3, ch: 2 }); // The delimiter row.
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!((c.head.line, c.head.ch), (4, 2), "Tab on the delimiter row goes to the next row's first cell, no spaces inserted");
        assert_eq!(d.to_text(), text);
        let mut c = edit::Caret::at(edit::Pos { line: 4, ch: 6 }); // The last cell.
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!(d.to_text(), "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n|  |  |\n");
        assert_eq!((c.head.line, c.head.ch), (5, 2));
        d.undo();
        assert_eq!(d.to_text(), text);
    }

    #[test]
    fn leaving_an_edited_table_realigns_once_and_an_undo_is_not_realigned_again() {
        let text = "|a|bb|\n|:-|-:|\n|ccc|d|\n";
        let mut d = edit::Document::from_text(text);
        let now = Instant::now();
        assert!(!realign_on_leave(&mut d, 0..3, text, now), "unchanged since entry: nothing");
        // The text when the caret entered; the user typed a "b".
        let entered = "|a|b|\n|:-|-:|\n|ccc|d|\n";
        assert!(realign_on_leave(&mut d, 0..3, entered, now));
        assert_eq!(d.to_text(), "| a   | bb |\n|:----|---:|\n| ccc |  d |\n");
        d.undo();
        assert_eq!(d.to_text(), text);
        assert!(!realign_on_leave(&mut d, 0..3, text, now), "after the undo the entry text is the restored text");
    }

    #[test]
    fn undo_back_to_the_original_text_and_out_does_not_realign() {
        let original = "intro\n\n|a|b|\n|-|-|\n|1|2|\n";
        let mut d = edit::Document::from_text(original);
        let now = Instant::now();
        let in_table = edit::Caret::at(edit::Pos { line: 2, ch: 2 });
        let away = edit::Caret::at(edit::Pos { line: 0, ch: 0 });
        let step = |d: &edit::Document, caret, entry, refresh| next_entry(&md_blocks::split(d.lines()), d.lines(), caret, entry, refresh, false);
        // The caret enters: the snapshot is the original text.
        let (realign, entry) = step(&d, in_table, None, false);
        assert!(realign.is_none());
        assert_eq!(entry.as_ref().map(|(start, _)| *start), Some(2));
        // The user types, then leaves: the table is realigned.
        d.insert(in_table, "x", now);
        let typed = d.to_text();
        let (realign, entry) = step(&d, away, entry, false);
        let (range, snapshot) = realign.expect("the typed table is realigned on leave");
        assert!(realign_on_leave(&mut d, range, &snapshot, now));
        assert!(entry.is_none(), "the caret is in a paragraph");
        // Undo the realign: the caret is back in the table and the snapshot is the typed text.
        let c = d.undo().unwrap();
        assert_eq!(d.to_text(), typed);
        let (realign, entry) = step(&d, c, entry, true);
        assert!(realign.is_none());
        // Undo the typing: still in the table, the snapshot refreshed to the original text.
        let c = d.undo().unwrap();
        assert_eq!(d.to_text(), original);
        let (realign, entry) = step(&d, c, entry, true);
        assert!(realign.is_none());
        assert_eq!(entry.as_ref().map(|(_, text)| text.as_str()), Some("|a|b|\n|-|-|\n|1|2|\n"));
        // Leave: the original text is left alone and the redo history is kept.
        let (realign, entry) = step(&d, away, entry, false);
        assert!(realign.is_none(), "nothing to realign");
        assert!(entry.is_none());
        assert_eq!(d.to_text(), original);
        assert!(d.redo().is_some(), "the redo history is still there");
        assert_eq!(d.to_text(), typed);
    }

    #[test]
    fn the_realign_range_and_the_appended_row_end_at_the_last_table_row_when_definitions_follow() {
        let text = "intro\n\n|a|b|\n|---|---|\n|1|2|\n\n[ref]: https://x\n";
        let mut d = edit::Document::from_text(text);
        let now = Instant::now();
        let blocks = md_blocks::split(d.lines());
        let table = &blocks[md_blocks::block_at(&blocks, 2)];
        assert_eq!(table.kind, md_blocks::BlockKind::Table);
        assert!(table.lines.contains(&6), "the parser gives the definition to the table block");
        assert_eq!(table_range(d.lines(), table), 2..5);
        // Enter, type, leave: only the table's lines are realigned, the definition stays.
        let in_table = edit::Caret::at(edit::Pos { line: 2, ch: 1 });
        let away = edit::Caret::at(edit::Pos { line: 0, ch: 0 });
        let (_, entry) = next_entry(&blocks, d.lines(), in_table, None, false, false);
        d.insert(in_table, "x", now);
        let blocks = md_blocks::split(d.lines());
        let (realign, _) = next_entry(&blocks, d.lines(), away, entry, false, false);
        let (range, snapshot) = realign.expect("the typed table is realigned on leave");
        assert_eq!(range, 2..5);
        assert!(realign_on_leave(&mut d, range, &snapshot, now));
        assert_eq!(d.to_text(), "intro\n\n| xa | b |\n|----|---|\n| 1  | 2 |\n\n[ref]: https://x\n");
        // Tab from the last cell appends the row after the last table row, not after the definition.
        let mut d = edit::Document::from_text(text);
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret::at(edit::Pos { line: 4, ch: 3 });
        assert_eq!(table_tab(&mut d, &mut c, &blocks, true, now), Some(Outcome::Edited));
        assert_eq!(d.to_text(), "intro\n\n|a|b|\n|---|---|\n|1|2|\n|  |  |\n\n[ref]: https://x\n");
        assert_eq!((c.head.line, c.head.ch), (5, 2));
        // The blank line and the definition are not the table's rows: Tab there is a plain Tab.
        let mut d = edit::Document::from_text(text);
        let blocks = md_blocks::split(d.lines());
        for line in [5, 6] {
            let mut c = edit::Caret::at(edit::Pos { line, ch: 0 });
            assert_eq!(table_tab(&mut d, &mut c, &blocks, true, now), None, "line {line}");
        }
        assert_eq!(d.to_text(), text);
    }

    #[test]
    fn escape_out_of_an_edited_table_realigns() {
        let now = Instant::now();
        let mut d = edit::Document::from_text("intro\n\n|a|b|\n|-|-|\n|1|2|\n");
        let in_table = edit::Caret::at(edit::Pos { line: 2, ch: 1 });
        let (_, entry) = next_entry(&md_blocks::split(d.lines()), d.lines(), in_table, None, false, false);
        d.insert(in_table, "x", now);
        // Escape: the caret stays on the table's line but is no longer placed, and the grid draws
        // again, so the table counts as left.
        let (realign, entry) = next_entry(&md_blocks::split(d.lines()), d.lines(), in_table, entry, false, true);
        let (range, snapshot) = realign.expect("Escape realigns the edited table");
        assert_eq!(range, 2..5);
        assert!(entry.is_none(), "an unplaced caret is in no table");
        assert!(realign_on_leave(&mut d, range, &snapshot, now));
        assert_eq!(d.to_text(), "intro\n\n| xa | b |\n|----|---|\n| 1  | 2 |\n");
        // Placed again in the realigned table: a fresh snapshot, nothing to realign.
        let (realign, entry) = next_entry(&md_blocks::split(d.lines()), d.lines(), in_table, None, false, false);
        assert!(realign.is_none());
        assert_eq!(entry.map(|(start, _)| start), Some(2));
        // An unplaced caret takes no snapshot either.
        assert_eq!(next_entry(&md_blocks::split(d.lines()), d.lines(), in_table, None, false, true), (None, None));
    }

    #[test]
    fn next_entry_forgets_a_snapshot_whose_start_line_no_longer_starts_a_table() {
        let snapshot = Some((2, "|a|b|\n|-|-|\n|1|2|\n".to_string()));
        let away = edit::Caret::at(edit::Pos { line: 0, ch: 0 });
        // Enter before the header moved the table down a line, and the caret left it.
        let d = edit::Document::from_text("intro\n\n\n|a|b|\n|-|-|\n|1|2|\n");
        assert_eq!(next_entry(&md_blocks::split(d.lines()), d.lines(), away, snapshot.clone(), false, false), (None, None));
        // The delimiter row was broken: the block is a paragraph now, left as typed.
        let d = edit::Document::from_text("intro\n\n|a|b|\n|-x-|\n|1|2|\n");
        assert_eq!(next_entry(&md_blocks::split(d.lines()), d.lines(), away, snapshot.clone(), false, false), (None, None));
        // The same table still there and changed: it is realigned.
        let d = edit::Document::from_text("intro\n\n|a|bc|\n|-|-|\n|1|2|\n");
        let (realign, _) = next_entry(&md_blocks::split(d.lines()), d.lines(), away, snapshot, false, false);
        assert_eq!(realign.map(|(range, _)| range), Some(2..5));
    }

    #[test]
    fn keystrokes_map_to_editor_actions_the_mac_way() {
        let k = |s: &str| key_action(&gpui::Keystroke::parse(s).unwrap());
        assert_eq!(k("left"), Some(KeyAction::Move(Move::Left, false)));
        assert_eq!(k("shift-right"), Some(KeyAction::Move(Move::Right, true)));
        assert_eq!(k("alt-left"), Some(KeyAction::Move(Move::WordLeft, false)));
        assert_eq!(k("cmd-left"), Some(KeyAction::Move(Move::LineStart, false)));
        assert_eq!(k("cmd-shift-right"), Some(KeyAction::Move(Move::LineEnd, true)));
        assert_eq!(k("cmd-up"), Some(KeyAction::Move(Move::DocStart, false)));
        assert_eq!(k("pagedown"), Some(KeyAction::Move(Move::PageDown(0), false)), "the rows are filled in by the caller");
        assert_eq!(k("home"), Some(KeyAction::ScrollHome));
        assert_eq!(k("backspace"), Some(KeyAction::DeleteBack(Unit::Grapheme)));
        assert_eq!(k("alt-backspace"), Some(KeyAction::DeleteBack(Unit::Word)));
        assert_eq!(k("cmd-backspace"), Some(KeyAction::DeleteBack(Unit::LineStart)));
        assert_eq!(k("delete"), Some(KeyAction::DeleteFwd(Unit::Grapheme)));
        assert_eq!(k("enter"), Some(KeyAction::Newline));
        assert_eq!(k("shift-enter"), Some(KeyAction::BreakLine));
        assert_eq!(k("tab"), Some(KeyAction::Tab));
        assert_eq!(k("shift-tab"), Some(KeyAction::ShiftTab));
        assert_eq!(k("cmd-z"), Some(KeyAction::Undo));
        assert_eq!(k("cmd-shift-z"), Some(KeyAction::Redo));
        assert_eq!(k("cmd-a"), Some(KeyAction::SelectAll));
        assert_eq!(k("cmd-c"), Some(KeyAction::Copy));
        assert_eq!(k("cmd-v"), Some(KeyAction::Paste));
        assert_eq!(k("escape"), Some(KeyAction::Escape));
        assert_eq!(k("space"), None, "text comes through the input handler, not the key table");
        assert_eq!(k("a"), None);
        assert_eq!(k("cmd-k"), None, "unknown chords are left to the keymap");
    }

    #[test]
    fn enter_continues_a_list_tab_indents_the_item_and_a_fence_closes_itself() {
        let now = Instant::now();
        let mut d = edit::Document::from_text("- one\n- two");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 1, ch: 5 }, anchor: Pos { line: 1, ch: 5 }, goal_col: None };
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now), Outcome::Edited);
        assert_eq!(d.lines()[2], "- ");
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines()[2], "", "Enter on an empty item ends the list");
        c.head = Pos { line: 1, ch: 0 }; c.anchor = c.head;
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!(d.lines()[1], "  - two");
        apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
        assert_eq!(d.lines()[1], "- two");
        c.head = Pos { line: 0, ch: 5 }; c.anchor = c.head;
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now), Outcome::Nothing, "the list's first item has nothing to go under");
        assert_eq!(d.lines()[0], "- one");
        // A fence: three backticks and Enter open it and close it when nothing below closes it.
        let mut d = edit::Document::from_text("```rust\n\nlater");
        let blocks = md_blocks::split(edit::Document::from_text("\n\nlater").lines()); // the structure before the backticks were typed
        let mut c = edit::Caret { head: Pos { line: 0, ch: 7 }, anchor: Pos { line: 0, ch: 7 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines(), &["```rust".to_string(), String::new(), "```".to_string(), String::new(), "later".to_string()][..]);
        assert_eq!(c.head, Pos { line: 1, ch: 0 });
        let mut d = edit::Document::from_text("```\ncode\n```");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 3 }, anchor: Pos { line: 0, ch: 3 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines().len(), 4, "a fence that is already closed gets a plain newline");
        let mut d = edit::Document::from_text("```\ncode\n```");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 2, ch: 3 }, anchor: Pos { line: 2, ch: 3 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines(), &["```".to_string(), "code".to_string(), "```".to_string(), String::new()][..], "Enter after a closing fence is a plain newline");
        // A fence typed above an existing closed block closes itself; the old block stays whole.
        let mut d = edit::Document::from_text("```rust\n\n```\nold\n```");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 7 }, anchor: Pos { line: 0, ch: 7 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        let want = ["```rust", "", "```", "", "```", "old", "```"].map(String::from);
        assert_eq!((d.lines(), c.head), (&want[..], Pos { line: 1, ch: 0 }));
        // A lone new fence closes itself.
        let mut d = edit::Document::from_text("```\nlater");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 3 }, anchor: Pos { line: 0, ch: 3 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines(), &["```", "", "```", "later"].map(String::from)[..]);
        // Two balanced blocks: Enter after the first opener is a plain newline.
        let two = "```\na\n```\ntext\n```\nb\n```";
        let mut d = edit::Document::from_text(two);
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 3 }, anchor: Pos { line: 0, ch: 3 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        assert_eq!(d.lines(), &["```", "", "a", "```", "text", "```", "b", "```"].map(String::from)[..], "the fences stay balanced");
        // A new fence typed above the two blocks closes itself and leaves both whole.
        let mut d = edit::Document::from_text(&format!("```rust\n\n{two}"));
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 7 }, anchor: Pos { line: 0, ch: 7 }, goal_col: None };
        apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
        let want = ["```rust", "", "```", "", "```", "a", "```", "text", "```", "b", "```"].map(String::from);
        assert_eq!((d.lines(), c.head), (&want[..], Pos { line: 1, ch: 0 }));
        assert_eq!(normalize_paste("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn copy_cut_paste_and_the_scroll_keys_leave_their_outcome_to_the_caller() {
        let now = Instant::now();
        let mut d = edit::Document::from_text("one two\n```");
        let blocks = md_blocks::split(d.lines());
        let mut c = edit::Caret { head: Pos { line: 0, ch: 3 }, anchor: Pos { line: 0, ch: 0 }, goal_col: None };
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Copy, 20, now), Outcome::Copy("one".into()));
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Cut, 20, now), Outcome::Cut("one".into()));
        assert_eq!((d.lines()[0].as_str(), c.head, c.anchor), (" two", Pos { line: 0, ch: 0 }, Pos { line: 0, ch: 0 }));
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Copy, 20, now), Outcome::Nothing, "nothing selected, nothing copied");
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Paste, 20, now), Outcome::Paste);
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::ScrollEnd, 20, now), Outcome::Scroll(KeyAction::ScrollEnd));
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now), Outcome::Nothing, "Shift+Tab outside a list item does nothing");
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("     two", 4), "Tab outside an item is four spaces");
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Move(Move::PageDown(0), false), 20, now), Outcome::Moved);
        assert_eq!(c.head.line, 1, "the page is the caller's row count");
        assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Undo, 20, now), Outcome::Edited);
        assert_eq!(d.lines()[0], " two");
        let mut d = edit::Document::from_text("- a\n");
        let mut c = edit::Caret { head: Pos { line: 0, ch: 0 }, anchor: Pos { line: 0, ch: 0 }, goal_col: None };
        apply_action(&mut d, &mut c, &[], KeyAction::Newline, 20, now);
        assert_eq!(d.lines()[..2], ["".to_string(), "- a".to_string()], "Enter before the marker moves the item down whole");
        let mut d = edit::Document::from_text("~~~~\n");
        let mut c = edit::Caret { head: Pos { line: 0, ch: 4 }, anchor: Pos { line: 0, ch: 4 }, goal_col: None };
        apply_action(&mut d, &mut c, &[], KeyAction::Newline, 20, now);
        assert_eq!(d.lines()[..3], ["~~~~".to_string(), String::new(), "~~~~".to_string()], "a tilde fence closes with its own length");
    }

    #[test]
    fn before_the_caret_is_placed_the_scroll_keys_scroll_and_an_edit_places_it() {
        use PrePlacement::*;
        let k = |s: &str| pre_placement_action(key_action(&gpui::Keystroke::parse(s).unwrap()).unwrap());
        for s in ["pageup", "pagedown", "home", "end", "up", "down", "shift-down"] {
            assert_eq!(k(s), Scroll, "{s}");
        }
        for s in ["enter", "tab", "shift-tab", "backspace", "delete", "alt-backspace", "cmd-v", "cmd-x", "cmd-z", "cmd-shift-z", "cmd-a"] {
            assert_eq!(k(s), Place, "{s}");
        }
        for s in ["escape", "cmd-c", "left", "right", "alt-left", "cmd-right", "cmd-up", "cmd-s"] {
            assert_eq!(k(s), Ignore, "{s}");
        }
        assert!(!places_caret(KeyAction::Escape) && !places_caret(KeyAction::Copy) && !places_caret(KeyAction::ScrollEnd));
        assert!(places_caret(KeyAction::Move(Move::Left, false)) && places_caret(KeyAction::Newline));
    }

    #[test]
    fn utf16_offsets_are_relative_to_the_active_block() {
        let d = edit::Document::from_text("# T\n\na😀b\nc");
        let from = Pos { line: 2, ch: 0 };
        assert_eq!(d.pos_to_utf16(from, Pos { line: 2, ch: 2 }), 3, "the emoji is two UTF-16 units");
        assert_eq!(d.utf16_to_pos(from, 3), Pos { line: 2, ch: 2 });
        assert_eq!(d.utf16_to_pos(from, 5), Pos { line: 3, ch: 0 }, "the line break counts one unit");
        assert_eq!(d.utf16_to_pos(from, 99), Pos { line: 3, ch: 1 }, "clamped");
    }

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
    fn the_rows_and_the_cache_follow_the_blocks_through_an_edit_an_undo_and_a_reload() {
        let list = ListState::new(0, ListAlignment::Top, px(600.0));
        let mut cache: Vec<Option<md_view::Rendered>> = Vec::new();
        let mut doc = edit::Document::from_text("# T\n\npara\n\n- a\n- b\n");
        let mut blocks = md_blocks::split(doc.lines());
        reset_rows(&blocks, &mut cache, &list);
        let check = |blocks: &[md_blocks::Block], cache: &[Option<md_view::Rendered>], step: &str| {
            assert_eq!(list.item_count(), list_rows(blocks.len()), "{step}: a row per block and the tail");
            assert_eq!(cache.len(), blocks.len(), "{step}: a cache entry per block");
        };
        check(&blocks, &cache, "load");
        // An edit that adds a block in the middle.
        let old = doc.shared_lines();
        doc.insert(edit::Caret::at(Pos { line: 2, ch: 4 }), "\n\n```\ncode\n```", Instant::now());
        splice_rows(&mut blocks, &mut cache, &list, &old, doc.lines());
        assert_eq!(blocks, md_blocks::split(doc.lines()), "the spliced blocks are the document's");
        check(&blocks, &cache, "edit");
        let old = doc.shared_lines();
        doc.undo().unwrap();
        splice_rows(&mut blocks, &mut cache, &list, &old, doc.lines());
        assert_eq!(blocks, md_blocks::split(doc.lines()));
        check(&blocks, &cache, "undo");
        // A reload with a shorter file.
        blocks = md_blocks::split(edit::Document::from_text("one").lines());
        reset_rows(&blocks, &mut cache, &list);
        check(&blocks, &cache, "reload");
    }

    #[test]
    fn a_block_s_focus_is_in_its_own_bytes_and_the_selection_is_clipped_to_it() {
        let lines: Vec<String> = "a **b**\n\nc d\n".split('\n').map(String::from).collect();
        let blocks = md_blocks::split(&lines);
        let caret = edit::Caret { head: Pos { line: 2, ch: 1 }, anchor: Pos { line: 0, ch: 3 }, goal_col: None };
        let first = focus_of(&lines, &blocks[0], caret, &[], None, None);
        assert_eq!((first.offsets.clone(), first.caret, first.selection.clone()), (vec![3], None, Some(3..8)), "the anchor is here; the selection runs to the block's end");
        let second = focus_of(&lines, &blocks[1], caret, &[], None, None);
        assert_eq!((second.offsets.clone(), second.caret, second.selection.clone()), (vec![1], Some(1), Some(0..1)));
        let hit = Hit { line: 2, start: 2, end: 3 };
        let found = focus_of(&lines, &blocks[1], edit::Caret::at(Pos { line: 0, ch: 0 }), &[hit], Some(hit), None);
        assert_eq!((found.offsets.clone(), found.hits.clone(), found.caret), (vec![2, 3], vec![(2..3, true)], None), "the current hit's ends reveal");
        let marked = Pos { line: 2, ch: 0 }..Pos { line: 2, ch: 1 };
        assert_eq!(focus_of(&lines, &blocks[1], edit::Caret::at(Pos { line: 2, ch: 1 }), &[], None, Some(&marked)).marked, Some(0..1));
    }

    #[test]
    fn a_point_picks_the_first_laid_out_text_below_it_and_an_offset_the_last_item_starting_at_it() {
        let spans = [(0.0, 10.0), (10.0, 30.0), (40.0, 50.0)];
        assert_eq!(pick_by_y(&spans, 5.0), Some(0));
        assert_eq!(pick_by_y(&spans, 35.0), Some(2), "between two texts: the one below");
        assert_eq!(pick_by_y(&spans, 99.0), Some(2), "below them all: the last");
        assert_eq!(pick_by_y(&[], 1.0), None);
        let covers = [0..6, 6..11];
        assert_eq!(pick_cover(&covers, 6), Some(1), "an item's start is its own, not the end of the one above");
        assert_eq!(pick_cover(&covers, 3), Some(0));
        assert_eq!(pick_cover(&covers, 12), None);
    }

    fn md_doc(text: &str) -> (edit::Document, Vec<md_blocks::Block>) {
        let doc = edit::Document::from_text(text);
        let blocks = md_blocks::split(doc.lines());
        (doc, blocks)
    }

    #[test]
    fn in_a_revealed_item_moves_step_over_the_bullet_and_typing_lands_after_it() {
        let (doc, blocks) = md_doc("- one\n- two");
        let at = |line, ch| edit::Caret::at(Pos { line, ch });
        assert_eq!(move_snapped(&doc, &blocks, at(1, 2), edit::Move::Left, false).head, Pos { line: 0, ch: 5 });
        assert_eq!(move_snapped(&doc, &blocks, at(0, 5), edit::Move::Right, false).head, Pos { line: 1, ch: 2 });
        assert_eq!(move_snapped(&doc, &blocks, at(1, 4), edit::Move::LineStart, false).head, Pos { line: 1, ch: 2 });
        assert_eq!(snap_pos(doc.lines(), &blocks, Pos { line: 1, ch: 0 }, md_reveal::Dir::Right), Pos { line: 1, ch: 2 }, "typing with the caret in the marker inserts after it");
        let (doc, blocks) = md_doc("```\n- x\n```");
        assert_eq!(snap_pos(doc.lines(), &blocks, Pos { line: 1, ch: 0 }, md_reveal::Dir::Right), Pos { line: 1, ch: 0 }, "a code block is not revealed, so nothing is hidden in it");
        let (doc, blocks) = md_doc("para\n\n- a");
        let extended = move_snapped(&doc, &blocks, edit::Caret { head: Pos { line: 0, ch: 4 }, anchor: Pos { line: 0, ch: 0 }, goal_col: None }, edit::Move::Right, true);
        assert_eq!((extended.anchor, extended.head), (Pos { line: 0, ch: 0 }, Pos { line: 1, ch: 0 }), "Shift keeps the anchor");
    }

    #[test]
    fn backspace_at_an_item_s_text_start_removes_its_marker_in_one_undoable_step() {
        let now = Instant::now();
        let (mut doc, blocks) = md_doc("- [ ] a\n- b");
        let c = backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 0, ch: 6 }), now).expect("unmarked");
        assert_eq!((doc.lines().to_vec(), c.head), (vec!["a".to_string(), String::new(), "- b".to_string()], Pos { line: 0, ch: 0 }), "the item leaves the list as a paragraph");
        assert!(doc.undo().is_some());
        assert_eq!(doc.lines().to_vec(), vec!["- [ ] a".to_string(), "- b".to_string()], "one undo brings it back");
        let (mut doc, blocks) = md_doc("- a\n- b\n- c");
        let c = backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 1, ch: 2 }), now).expect("unmarked");
        assert_eq!((doc.lines().to_vec(), c.head), (["- a", "", "b", "", "- c"].map(String::from).to_vec(), Pos { line: 2, ch: 0 }), "a middle item splits the list");
        let (mut doc, blocks) = md_doc("- a\n  - b");
        let c = backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 1, ch: 4 }), now).expect("outdented");
        assert_eq!((doc.lines()[1].as_str(), c.head), ("- b", Pos { line: 1, ch: 2 }), "a nested item moves out a level and keeps its bullet");
        let (mut doc, blocks) = md_doc("- ab");
        assert_eq!(backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 0, ch: 3 }), now), None, "elsewhere it is an ordinary delete");
        let (mut doc, blocks) = md_doc("> q1\n> q2");
        let c = backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 0, ch: 2 }), now).expect("left the quote");
        assert_eq!((doc.lines().to_vec(), c.head), (vec!["q1".to_string(), "q2".to_string()], Pos { line: 0, ch: 0 }));
        let (mut doc, blocks) = md_doc("## h");
        assert_eq!(backspace_revealed(&mut doc, &blocks, edit::Caret::at(Pos { line: 0, ch: 3 }), now), None);
    }

    #[test]
    fn displayed_rows_are_the_runs_of_char_boundaries_at_one_height() {
        let ys = [0.0, 0.0, 0.0, 20.0, 20.0, 40.0];
        assert_eq!(rows_by_y("abcdef", |b| ys[b.min(5)]), vec![0..2, 3..4, 5..6]);
        assert_eq!(rows_by_y("", |_| 0.0), vec![0..0]);
    }

    /// The document after `keys` in the rendered view's break rules, from `text` with the caret at
    /// `at`, and the caret.
    fn broken(text: &str, at: Pos, shift: bool) -> (Vec<String>, Pos) {
        let mut doc = edit::Document::from_text(text);
        let blocks = md_blocks::split(doc.lines());
        let c = rendered_break(&mut doc, &blocks, edit::Caret::at(at), shift, Instant::now()).unwrap_or_else(|| edit::Caret::at(Pos { line: usize::MAX, ch: 0 }));
        (doc.lines().to_vec(), c.head)
    }

    fn lines_of(s: &[&str]) -> Vec<String> {
        s.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn enter_makes_a_new_paragraph_and_inside_a_quote_a_new_quoted_one() {
        assert_eq!(broken("one two", Pos { line: 0, ch: 3 }, false), (lines_of(&["one", "", " two"]), Pos { line: 2, ch: 0 }), "the paragraph splits");
        assert_eq!(broken("# Title", Pos { line: 0, ch: 7 }, false), (lines_of(&["# Title", "", ""]), Pos { line: 2, ch: 0 }), "after a heading, plain text");
        assert_eq!(broken("> one two", Pos { line: 0, ch: 5 }, false), (lines_of(&["> one", ">", ">  two"]), Pos { line: 2, ch: 2 }), "a new paragraph in the quote");
        assert_eq!(broken("> > deep", Pos { line: 0, ch: 8 }, false).0, lines_of(&["> > deep", "> >", "> > "]), "as deep as the line");
        assert_eq!(broken("> one\n> ", Pos { line: 1, ch: 2 }, false), (lines_of(&["> one", ""]), Pos { line: 1, ch: 0 }), "Enter on an empty quote line ends the quote");
        assert_eq!(broken("> one\n>\n> ", Pos { line: 2, ch: 2 }, false), (lines_of(&["> one", "", ""]), Pos { line: 2, ch: 0 }), "and takes the bare > line above with it");
        assert_eq!(broken("```\ncode\n```", Pos { line: 1, ch: 4 }, false).1, Pos { line: usize::MAX, ch: 0 }, "a code block keeps the plain break");
        assert_eq!(broken("- a", Pos { line: 0, ch: 3 }, false).1, Pos { line: usize::MAX, ch: 0 }, "a list keeps its continuation");
    }

    #[test]
    fn shift_enter_breaks_the_line_and_leaves_a_quote_or_a_code_block() {
        assert_eq!(broken("one two", Pos { line: 0, ch: 3 }, true), (lines_of(&["one\\", " two"]), Pos { line: 1, ch: 0 }), "a hard break, backslash style");
        assert_eq!(broken("- item text", Pos { line: 0, ch: 6 }, true), (lines_of(&["- item\\", "   text"]), Pos { line: 1, ch: 2 }), "the item's continuation indent");
        assert_eq!(broken("10. item", Pos { line: 0, ch: 8 }, true), (lines_of(&["10. item\\", "    "]), Pos { line: 1, ch: 4 }));
        assert_eq!(broken("> last", Pos { line: 0, ch: 3 }, true), (lines_of(&["> last", "", ""]), Pos { line: 2, ch: 0 }), "on the quote's last line, a paragraph after it");
        assert_eq!(broken("> one\n> two", Pos { line: 0, ch: 5 }, true), (lines_of(&["> one", "", "", "", "> two"]), Pos { line: 2, ch: 0 }), "in the middle, the quote splits around a paragraph");
        assert_eq!(broken("> one two\n> end", Pos { line: 0, ch: 5 }, true).0, lines_of(&["> one", "", "", "", ">  two", "> end"]), "the rest of the line stays quoted");
        assert_eq!(broken("```\ncode\n```\n\nafter", Pos { line: 1, ch: 2 }, true), (lines_of(&["```", "code", "```", "", "", "", "after"]), Pos { line: 4, ch: 0 }), "from the last code line, out of the block");
        assert_eq!(broken("```\na\nb\n```", Pos { line: 1, ch: 1 }, true).1, Pos { line: usize::MAX, ch: 0 }, "elsewhere in code, a plain break");
        assert_eq!(broken("# Title", Pos { line: 0, ch: 7 }, true).0, lines_of(&["# Title", "", ""]), "a heading has no hard break: a new paragraph");
        assert_eq!(broken("end\n", Pos { line: 1, ch: 0 }, true), (lines_of(&["end", "", ""]), Pos { line: 2, ch: 0 }), "on the blank line after the text, a line, not a lone backslash");
    }

    #[test]
    fn tab_nests_an_item_one_level_under_the_one_above_and_shift_tab_brings_it_back() {
        let now = Instant::now();
        let run = |text: &str, line: usize, ch: usize, action: KeyAction| {
            let mut d = edit::Document::from_text(text);
            let mut c = edit::Caret::at(Pos { line, ch });
            let out = apply_action(&mut d, &mut c, &[], action, 20, now);
            (d.lines().to_vec(), c.head, out)
        };
        assert_eq!(run("1. one\n2. two", 1, 4, KeyAction::Tab).0, lines_of(&["1. one", "   1. two"]), "under a numbered item, its content column, numbered 1 to start the sublist");
        assert_eq!(run("1. a\n   1. b\n2. c", 2, 4, KeyAction::Tab).0, lines_of(&["1. a", "   1. b", "   2. c"]), "after a sibling, the next number");
        assert_eq!(run("1. a\n   1. b", 1, 6, KeyAction::ShiftTab).0, lines_of(&["1. a", "2. b"]), "back out, after the item above");
        assert_eq!(run("- a\n- b", 1, 3, KeyAction::Tab).0, lines_of(&["- a", "  - b"]), "a bullet keeps its marker");
        assert_eq!(run("- a\n  - b", 1, 5, KeyAction::Tab).2, Outcome::Nothing, "one level under the item above at most");
        let (lines, head, _) = run("- a\n- b\n  more\n  - c\n- d", 1, 3, KeyAction::Tab);
        assert_eq!((lines, head), (lines_of(&["- a", "  - b", "    more", "    - c", "- d"]), Pos { line: 1, ch: 5 }), "its continuation lines and children move with it");
        assert_eq!(run("- a\n  - b\n    more", 2, 6, KeyAction::ShiftTab).0, lines_of(&["- a", "- b", "  more"]), "Shift+Tab from a continuation line, back to the parent's column");
        assert_eq!(run("- a\n- b", 1, 3, KeyAction::ShiftTab).2, Outcome::Nothing, "a top-level item stays");
        let mut d = edit::Document::from_text("- a\n  - ");
        let c = newline(&mut d, edit::Caret::at(Pos { line: 1, ch: 4 }), &[], now);
        assert_eq!((d.lines().to_vec(), c.head), (lines_of(&["- a", "- "]), Pos { line: 1, ch: 2 }), "Enter on an empty nested item moves it out a level");
    }

    #[test]
    fn a_slash_opens_the_menu_where_a_word_could_start_outside_code_and_tables() {
        let opens = |text: &str, line: usize, ch: usize| {
            let lines: Vec<String> = text.split('\n').map(String::from).collect();
            slash_opens(&lines, &md_blocks::split(&lines), line, ch)
        };
        assert!(opens("/", 0, 0), "a line of its own");
        assert!(opens("some text /", 0, 10), "after a space");
        assert!(!opens("and/or", 0, 3), "inside a word: a slash is text");
        assert!(opens("- item /", 0, 7));
        assert!(opens("# Head /", 0, 7));
        assert!(!opens("```\n/\n```", 1, 0), "in a code block");
        assert!(!opens("| a |\n|---|\n| / |", 2, 2), "in a table");
        assert!(opens("```\nx\n```\n\n/", 4, 0), "on a line after a code block");
    }

    #[test]
    fn the_caret_steps_over_a_code_block_s_fence_lines() {
        let (doc, blocks) = md_doc("para\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\nafter");
        let at = |line, ch| edit::Caret::at(Pos { line, ch });
        let lines = doc.lines();
        assert_eq!(snap_pos(lines, &blocks, Pos { line: 2, ch: 3 }, md_reveal::Dir::Here), Pos { line: 3, ch: 0 }, "an edit on the opening fence lands in the code");
        assert_eq!(snap_pos(lines, &blocks, Pos { line: 5, ch: 1 }, md_reveal::Dir::Here), Pos { line: 4, ch: 10 }, "on the closing fence, at the code's end");
        assert_eq!(move_snapped(&doc, &blocks, at(3, 0), edit::Move::Left, false).head, Pos { line: 1, ch: 0 }, "Left from the code's start goes above the block");
        assert_eq!(move_snapped(&doc, &blocks, at(4, 10), edit::Move::Right, false).head, Pos { line: 6, ch: 0 }, "Right from the code's end goes below it");
        let down = md_view::navigate(&doc, &blocks, at(4, 3), md_view::Dir::Down, false);
        assert_eq!(down.head, Pos { line: 7, ch: 0 }, "Down from the last code line leaves the block");
        let up = md_view::navigate(&doc, &blocks, at(3, 3), md_view::Dir::Up, false);
        assert_eq!(up.head, Pos { line: 0, ch: 4 }, "Up from the first leaves it");
        let into = md_view::navigate(&doc, &blocks, at(0, 2), md_view::Dir::Down, false);
        assert_eq!(into.head, Pos { line: 3, ch: 0 }, "Down into the block lands on its first code line");
        let back = md_view::navigate(&doc, &blocks, at(7, 2), md_view::Dir::Up, false);
        assert_eq!(back.head, Pos { line: 4, ch: 10 }, "Up into it, at its last code line's end");
    }

    #[test]
    fn a_find_hit_becomes_the_selection() {
        let lines = vec!["caf\u{e9} cafe".to_string()];
        let hits = find_hits(&lines, "cafe");
        let c = hit_selection(&lines, hits[0]).unwrap();
        assert_eq!((c.anchor, c.head), (Pos { line: 0, ch: 5 }, Pos { line: 0, ch: 9 }), "byte offsets become characters");
        assert_eq!(hit_selection(&lines, Hit { line: 1, start: 0, end: 1 }), None, "a line that is gone");
        assert_eq!(hit_selection(&lines, Hit { line: 0, start: 4, end: 5 }), None, "inside a character");
        assert_eq!(hit_selection(&lines, Hit { line: 0, start: 0, end: 99 }), None, "past the line's end");
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

    #[test]
    fn markdown_is_known_by_extension() {
        for p in ["a.md", "b.MD", "c.markdown", "d.mdx"] {
            assert!(is_markdown(Path::new(p)), "{p}");
        }
        for p in ["a.rs", "README", "x.md.bak", "y.txt"] {
            assert!(!is_markdown(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn a_save_restores_the_file_s_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.md");
        std::fs::write(&path, "\u{feff}# T\r\n\r\ntext").unwrap();
        let mut d = edit::Document::from_text(&std::fs::read_to_string(&path).unwrap());
        assert!(d.bom && d.lines()[0] == "# T", "the BOM is a flag, not text");
        d.insert(edit::Caret::default(), "X", std::time::Instant::now());
        let fp = crate::config_watch::fingerprint(&path);
        save_text(&path, &d.to_text(), fp).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), "\u{feff}X# T\r\n\r\ntext".as_bytes());
    }

    #[test]
    fn a_save_is_atomic_keeps_permissions_follows_a_symlink_and_refuses_a_changed_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.md");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let fp = crate::config_watch::fingerprint(&link);
        let after = save_text(&link, "new", fp).unwrap();
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new", "written through the link");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
        assert_eq!(Some(after), crate::config_watch::fingerprint(&link), "the returned fingerprint is the file's");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let after2 = save_text(&real, "newer", Some(after)).unwrap();
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600, "permissions copied onto the renamed file");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "no temp file left behind");
        std::fs::write(&real, "someone else").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(matches!(save_text(&real, "mine", Some(after2)), Err(SaveError::Changed)), "another writer got there first");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "someone else", "and their file is untouched");
        assert!(save_text(&real, "forced", None).is_ok(), "no expectation: Keep mine");
    }

    #[test]
    fn an_unwritable_target_leaves_the_file_intact() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("ro");
        std::fs::create_dir(&sub).unwrap();
        let path = sub.join("a.md");
        std::fs::write(&path, "keep").unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o500)).unwrap();
        let fp = crate::config_watch::fingerprint(&path);
        let err = save_text(&path, "lost?", fp);
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(err, Err(SaveError::Io(_))), "{err:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 1, "no temp file left behind");
    }

    #[test]
    fn a_read_only_file_is_neither_replaced_nor_edited() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ro.md");
        std::fs::write(&path, "# keep\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let fp = crate::config_watch::fingerprint(&path);
        let err = save_text(&path, "lost?", fp);
        assert!(matches!(&err, Err(SaveError::Io(e)) if e.to_string() == "the file is read-only"), "{err:?}");
        assert!(matches!(save_text(&path, "lost?", None), Err(SaveError::Io(_))), "Keep mine does not replace it either");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# keep\n");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o444, "the file is the same one");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file left behind");
        let l = read_file(&path, None, None, 1 << 20, false).unwrap();
        assert!(l.doc.is_none(), "shown, not edited");
        assert_eq!(l.read_only, Some(ReadOnlyWhy::NotWritable));
        assert!(matches!(l.content, Content::Text { ref lines, .. } if lines[0] == "# keep"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn a_superseded_save_skips_its_last_step_and_leaves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "newer, from the close").unwrap();
        let fp = crate::config_watch::fingerprint(&path);
        let skipped = save_text_guarded(&path, "older", fp, || None::<()>);
        assert!(matches!(skipped, Err(SaveError::Superseded)), "{skipped:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer, from the close", "the rename never happened");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file left behind");
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(matches!(save_text_guarded(&link, "older", None, || None::<()>), Err(SaveError::Superseded)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer, from the close", "nor the write through the link");
        let mut gated = false;
        save_text_guarded(&path, "through the gate", fp, || {
            gated = true;
            Some(())
        })
        .unwrap();
        assert!(gated && std::fs::read_to_string(&path).unwrap() == "through the gate");
    }

    #[test]
    fn an_abandoned_flight_stops_the_writer_s_rename() {
        let flight = SaveFlight::default();
        assert!(flight.wait_or_abandon(Duration::from_millis(1)).is_none(), "still running: abandoned");
        assert!(flight.gate().is_none(), "the writer's gate is closed");
        let done = SaveFlight::default();
        done.finish(Some((1, 2)));
        assert_eq!(done.wait_or_abandon(Duration::from_millis(1)), Some(Some((1, 2))), "a finished write is reported, not abandoned");
        assert!(done.gate().is_some());
    }

    #[test]
    fn the_disk_change_rule_ignores_our_own_write() {
        let ours = Some((1, 10));
        assert_eq!(disk_change(ours, ours, true, false), DiskChange::Ignore, "our own write seen while dirty again");
        assert_eq!(disk_change(Some((2, 10)), ours, false, true), DiskChange::Ignore, "a save in flight: re-check when it lands");
        assert_eq!(disk_change(Some((2, 10)), ours, true, false), DiskChange::Conflict);
        assert_eq!(disk_change(Some((2, 10)), ours, false, false), DiskChange::Reload);
        assert_eq!(disk_change(None, ours, false, false), DiskChange::Reload, "the file went away: reload shows why");
    }

    #[test]
    fn a_timer_after_reload_does_not_write() {
        assert!(autosave_due(3, 3, true, false, false, false));
        assert!(!autosave_due(3, 4, true, false, false, false), "a newer edit rescheduled");
        assert!(!autosave_due(3, 3, false, false, false, false), "nothing dirty: Reload bumped save_seq and cleared dirty");
        assert!(!autosave_due(3, 3, true, true, false, false), "a conflict waits for the user");
        assert!(!autosave_due(3, 3, true, false, true, false), "a save in flight: save_again instead");
        assert!(!autosave_due(3, 3, true, false, false, true), "an IME composition in progress");
    }

    #[test]
    fn the_conflict_buttons_do_what_they_say() {
        // The pure state machine behind resolve_conflict: (conflict, dirty, save_seq) before and after.
        let s = ConflictState { conflict: true, dirty: true, save_seq: 7 };
        assert_eq!(s.resolve(true), (ConflictState { conflict: false, dirty: true, save_seq: 7 }, ConflictAction::SaveNow), "Keep mine saves at once, adopting the disk fingerprint");
        assert_eq!(s.resolve(false), (ConflictState { conflict: false, dirty: false, save_seq: 8 }, ConflictAction::Reload), "Reload drops the edits and bumps the sequence so a pending timer stays quiet");
    }

    #[test]
    fn the_same_text_is_the_same_lines_ending_mark_and_final_break() {
        let d = edit::Document::from_text;
        assert!(same_text(&d("# a\nb\n"), &d("# a\nb\n")));
        assert!(!same_text(&d("# a\nb\n"), &d("# a\nB\n")));
        assert!(!same_text(&d("a\nb\n"), &d("a\r\nb\r\n")), "the line ending counts");
        assert!(!same_text(&d("a"), &d("\u{feff}a")), "the byte order mark counts");
        assert!(!same_text(&d("a"), &d("a\n")), "the final line break counts");
    }

    #[test]
    fn a_recovery_file_is_named_after_the_file_and_the_time() {
        let dir = Path::new("/support/recovered");
        assert_eq!(recovery_path(dir, Path::new("/p/docs/notes.md"), 1_760_000_000, 0), PathBuf::from("/support/recovered/notes-1760000000.md"));
        assert_eq!(recovery_path(dir, Path::new("/p/a.markdown"), 5, 2), PathBuf::from("/support/recovered/a-5-2.md"), "a later attempt is numbered");
        assert_eq!(recovery_path(dir, Path::new("/"), 5, 0), PathBuf::from("/support/recovered/untitled-5.md"));
    }

    #[test]
    fn unsaved_edits_are_written_to_a_new_recovery_file_never_over_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("recovered");
        let source = Path::new("/elsewhere/notes.md");
        let first = write_recovery(&dir, source, "mine", 7).unwrap();
        assert_eq!(first, dir.join("notes-7.md"), "the folder is made on demand");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "mine");
        let second = write_recovery(&dir, Path::new("/other/notes.md"), "theirs", 7).unwrap();
        assert_eq!(second, dir.join("notes-7-1.md"), "the same name in the same second gets its own file");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "mine", "and the first is untouched");
    }

    #[test]
    fn markdown_loads_as_a_document_only_when_it_is_clean_utf8_with_one_ending() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.md"); std::fs::write(&ok, "# a\n\tb\n").unwrap();
        let l = read_file(&ok, None, None, 1 << 20, false).unwrap();
        assert_eq!(l.doc.as_ref().unwrap().lines(), &["# a".to_string(), "\tb".to_string(), String::new()][..], "the document keeps its tab and its phantom last line");
        assert_eq!(l.read_only, None);
        let bad = dir.path().join("bad.md"); std::fs::write(&bad, b"# a\n\xff\n").unwrap();
        let l = read_file(&bad, None, None, 1 << 20, false).unwrap();
        assert!(l.doc.is_none()); assert_eq!(l.read_only, Some(ReadOnlyWhy::NotUtf8));
        let mixed = dir.path().join("mixed.md"); std::fs::write(&mixed, "a\r\nb\nc").unwrap();
        let l = read_file(&mixed, None, None, 1 << 20, false).unwrap();
        assert!(l.doc.is_none()); assert_eq!(l.read_only, Some(ReadOnlyWhy::MixedEndings));
        for (name, text) in [("cr.md", "a\rb\rc"), ("tail.md", "a\nb\r"), ("crlf-cr.md", "a\r\nb\rc")] {
            let p = dir.path().join(name);
            std::fs::write(&p, text).unwrap();
            let l = read_file(&p, None, None, 1 << 20, false).unwrap();
            assert!(l.doc.is_none() && l.read_only == Some(ReadOnlyWhy::MixedEndings), "a lone CR in {text:?} is read-only, not dropped");
        }
        let again = read_file(&ok, None, None, 1 << 20, false).unwrap();
        assert_eq!(again.blocks, md_blocks::split(again.doc.as_ref().unwrap().lines()), "the blocks come with the load");
        assert!(!again.blocks.is_empty());
        let rs = dir.path().join("x.rs"); std::fs::write(&rs, "fn main() {}\n").unwrap();
        assert!(read_file(&rs, None, None, 1 << 20, false).unwrap().doc.is_none(), "not markdown: no document");
    }

    #[test]
    fn a_poster_cmd_click_asks_for_a_video_tab() {
        use crate::media::Media;
        assert_eq!(poster_press(true, Some(Media::Video), None, false), PosterPress::OpenVideo, "a local video plays in a tab of its own");
        assert_eq!(poster_press(false, Some(Media::Video), None, false), PosterPress::Caret, "a plain press puts the caret in the block");
        assert_eq!(poster_press(false, Some(Media::Video), Some("notes.md"), false), PosterPress::Caret);
        assert_eq!(poster_press(true, Some(Media::Video), Some("notes.md"), false), PosterPress::Link, "the link around it wins");
        assert_eq!(poster_press(true, Some(Media::Audio), None, false), PosterPress::QuickTime, "audio stays with QuickTime Player");
        assert_eq!(poster_press(true, None, None, false), PosterPress::Reveal, "a local webm is shown in Finder");
        assert_eq!(poster_press(true, Some(Media::Video), None, true), PosterPress::Link, "remote video is a web address");
        assert_eq!(poster_press(true, None, None, true), PosterPress::Link);
    }
}
