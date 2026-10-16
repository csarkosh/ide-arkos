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
use crate::md_table;
use crate::md_view;
use crate::motion::{self, Fades};
use crate::palette::Rgb;
use crate::scroll::ScrollAnimator;
use crate::theme::Theme;
use crate::tree::{PaneId, Tab, TabId, ViewMode};
use crate::viewer;
use crate::viewer_element::{self as element, RawLayout};
use crate::Root;
use gpui::{
    div, prelude::*, px, App, Bounds, ClickEvent, ClipboardItem, Context, Div, Entity, EntityId, EntityInputHandler, EventEmitter, FocusHandle, KeyDownEvent, Keystroke, ListAlignment,
    ListOffset, ListState, MouseButton, MouseDownEvent, Pixels, Point, SharedString, UTF16Selection, Window,
};
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
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
    OpenPath(PathBuf),
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
    /// The table block the caret is in, in the rendered view: its start line and its text when the
    /// caret entered it (`table_text`). Leaving it realigns its pipes when the text differs
    /// (`note_caret_moved`). Undo, redo and `edit_elsewhere` leave it as it is.
    table_entry: Option<(usize, String)>,
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
            caret_placed: false,
            pending_append: false,
            compose: None,
            selecting: false,
            table_entry: None,
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
        if let Some(line) = old_top_line.filter(|_| !self.blocks.is_empty()) {
            self.list_state.scroll_to(ListOffset { item_ix: md_blocks::block_at(&self.blocks, line), offset_in_item: px(0.0) });
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
            self.reveal_caret();
            self.note_caret_moved(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.finish_composition(cx);
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        if let Some(caret) = doc.redo() {
            self.caret = caret;
            self.changed(old_lines, cx);
            self.reveal_caret();
            self.note_caret_moved(cx);
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
        self.content = Content::Text { lines: new_lines, spans: Vec::new() };
        // The marks point at lines that may have moved; the next save computes them again.
        self.diff = None;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            // The old hits point into the old lines.
            f.hits.clear();
        }
        self.search(false, cx);
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
            self.list_state.scroll_to_reveal_item(md_blocks::block_at(&self.blocks, h.line));
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
        self.note_caret_moved(cx);
        handled
    }

    fn run_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.doc.is_none() {
            return false;
        }
        let Some(action) = key_action(ks) else { return false };
        // A key the input method passed on ends its composition, keeping the text.
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        // A paragraph a click on the tail promised: an edit writes it first (before Enter reads
        // the structure), a move or an undo lets it go.
        if self.pending_append {
            match action {
                KeyAction::DeleteBack(_) | KeyAction::DeleteFwd(_) | KeyAction::Newline | KeyAction::Tab | KeyAction::ShiftTab | KeyAction::Cut | KeyAction::Paste => self.take_pending_append(cx),
                KeyAction::Copy | KeyAction::Save | KeyAction::Escape | KeyAction::ScrollHome | KeyAction::ScrollEnd => {}
                // The rendered view's page keys only scroll.
                KeyAction::Move(edit::Move::PageUp(_) | edit::Move::PageDown(_), _) if self.rendered() => {}
                KeyAction::Move(..) | KeyAction::SelectAll | KeyAction::Undo | KeyAction::Redo => self.pending_append = false,
            }
        }
        if self.rendered() && self.rendered_key(action, cx) {
            return true;
        }
        match action {
            KeyAction::Save => self.save_command(cx),
            // Escape undoes one thing at a time: the find bar closes, a selection collapses, and
            // in the rendered view the active block renders again until the caret is placed anew.
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
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = normalize_paste(&text);
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
                let reads_blocks = action == KeyAction::Newline || (self.rendered() && matches!(action, KeyAction::Tab | KeyAction::ShiftTab));
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
                let dir = if m == edit::Move::Up { md_view::Dir::Up } else { md_view::Dir::Down };
                let Some(doc) = self.doc.as_mut() else { return false };
                doc.break_coalescing();
                self.caret = md_view::navigate(doc, &self.blocks, self.caret, dir, select);
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
        let caret_y = self
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
            })
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
        self.note_caret_moved(cx);
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

    /// The document position under a window point, through the last layout.
    pub fn pos_at_point(&self, at: Point<Pixels>, window: &mut Window) -> Option<edit::Pos> {
        if self.rendered() {
            // The raw block at that height, else the first one below it, else the last one.
            let doc = self.doc.as_ref()?;
            let b = self.raw_blocks.iter().find(|b| at.y < b.bottom()).or(self.raw_blocks.last())?;
            return Some(b.pos_at(doc.lines(), at, window));
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
            // Only a raw block on screen knows where its text is.
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
        if self.marked.is_some() {
            self.finish_composition(cx);
        }
        let Some(doc) = self.doc.as_mut() else { return };
        doc.break_coalescing();
        let p = doc.clamp(p);
        let anchor = if extend { doc.clamp(self.caret.anchor) } else { p };
        self.caret = edit::Caret { head: p, anchor, goal_col: None };
        self.selecting = true;
        self.caret_placed = true;
        self.pending_append = false;
        self.note_caret_moved(cx);
        cx.notify();
    }

    /// A press on a cell of rendered table block `block`: the cell's source row (`row`, a line
    /// within the block) and column (`cell`), and the char offset into its text the cell's layout
    /// found under the pointer (`ch`). The caret goes there as for any press (Shift extends the
    /// selection), so the block renders raw.
    pub fn click_table_cell(&mut self, block: usize, row: usize, cell: usize, ch: usize, extend: bool, cx: &mut Context<Self>) {
        let (Some(doc), Some(b)) = (self.doc.as_ref(), self.blocks.get(block)) else { return };
        let Some(p) = table_cell_pos(doc.lines(), b, md_table::CellPos { row, cell }, ch) else { return };
        self.place_caret(p, extend, cx);
    }

    /// After the caret moved (a key, a press, a click on the tail, an undo or a redo): in the
    /// rendered view, when the caret and its selection have left the table block `table_entry`
    /// holds, that table's pipes are realigned if its text differs from the text it had when the
    /// caret entered (`realign_on_leave`, one undo step); then `table_entry` holds the caret's
    /// block when it is a table. Comparing with the entry text rather than keeping an edited flag
    /// means an undo that puts the caret back in the table and a leave again leave the restored
    /// text alone. Outside the rendered view nothing is realigned and the entry is forgotten.
    fn note_caret_moved(&mut self, cx: &mut Context<Self>) {
        if !self.rendered() {
            self.table_entry = None;
            return;
        }
        let touches = |blocks: &[md_blocks::Block], c: edit::Caret, start: usize| {
            [c.head.line, c.anchor.line].iter().any(|&l| blocks.get(md_blocks::block_at(blocks, l)).is_some_and(|b| b.lines.start == start))
        };
        if let Some((start, entry)) = self.table_entry.take() {
            if touches(&self.blocks, self.caret, start) {
                self.table_entry = Some((start, entry));
                return;
            }
            let Some(doc) = self.doc.as_ref() else { return };
            let range = self
                .blocks
                .get(md_blocks::block_at(&self.blocks, start))
                .filter(|b| b.lines.start == start && b.kind == md_blocks::BlockKind::Table)
                .map(|b| table_range(doc.lines(), b));
            if let Some(range) = range {
                self.edit_elsewhere(
                    |doc, caret, now| {
                        realign_on_leave(doc, range, &entry, now);
                        caret
                    },
                    cx,
                );
            }
        }
        let Some(doc) = self.doc.as_ref() else { return };
        self.table_entry = self
            .blocks
            .get(self.active_block())
            .filter(|b| b.kind == md_blocks::BlockKind::Table)
            .map(|b| (b.lines.start, table_text(doc.lines(), table_range(doc.lines(), b))));
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
        if self.field().is_none() {
            return;
        }
        let range = if self.place_for_input(cx) { None } else { range };
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
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, new_text: &str, new_selected_range: Option<Range<usize>>, _window: &mut Window, cx: &mut Context<Self>) {
        if self.field().is_none() {
            return;
        }
        let range = if self.place_for_input(cx) { None } else { range };
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
        let line_h = px(if self.rendered() { self.raw_blocks.first()?.raw.line_h } else { self.last_bounds.as_ref()?.line_h });
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
                        let spans = h.highlight(path, &lines);
                        (Content::Text { lines: Arc::new(lines), spans }, None)
                    }
                    // The grammars are still loading: plain for now, coloured when they land.
                    None => (plain(lines), None),
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
    Some(Loaded { content, fingerprint, plain: why_plain, diff, doc, read_only, blocks })
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
fn table_cell_pos(lines: &[String], block: &md_blocks::Block, pos: md_table::CellPos, ch: usize) -> Option<edit::Pos> {
    let source = md_view::block_source(lines, block);
    let (line, start) = md_table::cell_start(&source, pos)?;
    let line = block.lines.start + line;
    Some(edit::Pos { line, ch: (start + ch).min(lines.get(line)?.chars().count()) })
}

/// The lines of table block `block` down to its last line with text, the lines `realign` rewrites.
fn table_range(lines: &[String], block: &md_blocks::Block) -> Range<usize> {
    block.lines.start..md_blocks::pos_of_offset(block, lines, usize::MAX).line + 1
}

/// Lines `range`, each followed by a newline: a table's text as `table_entry` keeps it.
fn table_text(lines: &[String], range: Range<usize>) -> String {
    lines[range].iter().map(|l| format!("{l}\n")).collect()
}

/// The caret left a table whose lines are `lines` (down to its last line with text): when their
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
/// a table with no body) appends a row of empty cells after the block's last line with text, one
/// undo step, and lands in its first cell. Spaces are never inserted. `None` when the caret is not
/// in a table's rows (another block, a table that does not parse, the blank lines after it).
fn table_tab(doc: &mut edit::Document, caret: &mut edit::Caret, blocks: &[md_blocks::Block], forward: bool, now: Instant) -> Option<Outcome> {
    use md_table::CellPos;
    let head = doc.clamp(caret.head);
    let block = blocks.get(md_blocks::block_at(blocks, head.line)).filter(|b| b.kind == md_blocks::BlockKind::Table)?;
    let source = md_view::block_source(doc.lines(), block);
    md_table::parse(&source)?;
    let end = md_blocks::pos_of_offset(block, doc.lines(), usize::MAX);
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
        ("enter", false, false) => Some(K::Newline),
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
        K::DeleteBack(_) | K::DeleteFwd(_) | K::Newline | K::Tab | K::ShiftTab | K::Undo | K::Redo | K::Cut | K::Paste | K::SelectAll => PrePlacement::Place,
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

/// Tab and Shift+Tab on a list item line: two leading spaces added or taken away, the caret and
/// the anchor moving with the text. `None` when the caret's line is not a list item.
fn indent_item(doc: &mut edit::Document, caret: &mut edit::Caret, deeper: bool, now: Instant) -> Option<Outcome> {
    let line_no = doc.clamp(caret.head).line;
    let line = doc.lines()[line_no].clone();
    md_blocks::list_continuation(&line)?;
    let start = edit::Pos { line: line_no, ch: 0 };
    let shift = |p: &mut edit::Pos, by: isize| {
        if p.line == line_no {
            p.ch = (p.ch as isize + by).max(0) as usize;
        }
    };
    if deeper {
        doc.replace_range(start, start, "  ", now);
        shift(&mut caret.head, 2);
        shift(&mut caret.anchor, 2);
    } else {
        let spaces = line.chars().take(2).take_while(|&c| c == ' ').count();
        let n = if spaces == 0 && line.starts_with('\t') { 1 } else { spaces };
        if n == 0 {
            return Some(Outcome::Nothing);
        }
        doc.replace_range(start, edit::Pos { line: line_no, ch: n }, "", now);
        shift(&mut caret.head, -(n as isize));
        shift(&mut caret.anchor, -(n as isize));
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
        let mode = match self.tree.locate(tab).and_then(|(pane, idx)| self.tree.pane(pane)?.tabs.get(idx)) {
            Some(Tab::Viewer { mode, .. }) => *mode,
            _ => ViewMode::default(),
        };
        let v = cx.new(|cx| {
            let mut v = Viewer::new(path.to_path_buf(), font_size, line_height, cx);
            v.mode = mode;
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
                ViewerEvent::OpenPath(path) => root.open_viewer(path, true, cx),
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

    /// Flips a markdown tab between rendered and raw, in the layout (saved) and in its viewer.
    pub fn toggle_markdown_view_for(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some((pane, idx)) = self.tree.locate(tab) else { return };
        let Some(Tab::Viewer { path, mode }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() else { return };
        if !is_markdown(&path) {
            return;
        }
        let next = match mode {
            ViewMode::Rendered => ViewMode::Raw,
            ViewMode::Raw => ViewMode::Rendered,
        };
        self.tree.set_viewer_mode(pane, idx, next);
        if let Some(v) = self.viewers.get(&tab).cloned() {
            v.update(cx, |v, cx| v.set_mode(next, cx));
        }
        self.save(cx);
        cx.notify();
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
    use crate::edit::{Move, Pos, Unit};

    #[test]
    fn a_press_on_a_table_cell_maps_to_the_cell_s_text_in_the_source() {
        let d = edit::Document::from_text("intro\n\n| a | b |\n|---|---|\n| one | |\n");
        let blocks = md_blocks::split(d.lines());
        let table = &blocks[md_blocks::block_at(&blocks, 2)];
        assert_eq!(table.kind, md_blocks::BlockKind::Table);
        let at = |row, cell, ch| table_cell_pos(d.lines(), table, md_table::CellPos { row, cell }, ch);
        assert_eq!(at(2, 0, 2), Some(Pos { line: 4, ch: 4 }), "two chars into \"one\"");
        assert_eq!(at(2, 1, 0), Some(Pos { line: 4, ch: 8 }), "an empty cell: after its pipe and a space, clamped to the next pipe");
        assert_eq!(at(0, 1, 5), Some(Pos { line: 2, ch: 9 }), "past the text: at most the line's end");
        assert_eq!(at(9, 0, 0), None, "no such row");
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
        apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("  - one", 7), "Tab anywhere in an item indents the item and carries the caret");
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
}
