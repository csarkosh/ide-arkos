//! A read-only viewer tab (spec 6): the file loaded once off the UI thread, classified (text,
//! too big to highlight, binary), split into lines and highlighted; a find over the lines; a
//! scroll position in lines (in display rows while the inline diff shows deleted lines);
//! reloaded when the poll sees the file change. A file in a repository carries its diff
//! against HEAD (`diff`), recomputed on every load and on the ten-second timer.
//!
//! A markdown file that is clean UTF-8 with one kind of line ending is also an editable
//! `edit::Document`: edits mark the tab dirty and autosave a second later through `save_text`,
//! and a change on disk either reloads the file or, over unsaved edits, raises a conflict.

use crate::config_watch::fingerprint;
use crate::diff::{self, DiffMarks};
use crate::edit;
use crate::find::{FindLook, BAR_TOP};
use crate::highlight::{self, Highlighter};
use crate::line_edit::{self, Edit, LineEdit};
use crate::md_blocks;
use crate::motion::{self, Fades};
use crate::palette::Rgb;
use crate::scroll::ScrollAnimator;
use crate::theme::Theme;
use crate::tree::{PaneId, Tab, TabId, ViewMode};
use crate::viewer;
use crate::Root;
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Div, Entity, EntityId, EventEmitter, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, SharedString};
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
/// Why a tab with a conflict does not close, until the banner says it (Task 7).
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
}

/// What the viewer tells `Root`, which keeps the tab bar and the preview tab in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerEvent {
    /// The document changed: the tab is dirty and is no longer a preview.
    Edited,
    Saved,
    SaveFailed,
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
    /// The editable document, built on the background thread for a markdown file that is strict
    /// UTF-8 with one kind of line ending (a byte order mark is set aside in `Document.bom`).
    pub doc: Option<edit::Document>,
    /// Why a markdown file is shown read-only; `None` for every other file.
    pub read_only: Option<ReadOnlyWhy>,
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
        match (self.doc.take(), loaded.doc) {
            // The same text again: the old document stays, and with it the undo history.
            (Some(old), Some(new)) if old.to_text() == new.to_text() => {
                self.content = Content::Text { lines: old.shared_lines(), spans: Vec::new() };
                self.doc = Some(old);
            }
            (_, new) => self.doc = new,
        }
        match &self.doc {
            Some(doc) => {
                self.caret = edit::Caret { head: doc.clamp(self.caret.head), anchor: doc.clamp(self.caret.anchor), goal_col: None };
                self.blocks = md_blocks::split(doc.lines());
            }
            None => {
                self.caret = edit::Caret::default();
                self.blocks.clear();
            }
        }
        // What is shown is what is on disk now.
        self.dirty = false;
        self.conflict = false;
        self.save_error = None;
        self.marked = None;
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
    /// Nothing happens without a document.
    #[allow(dead_code)] // Used from Task 4 on.
    pub fn edit(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        self.caret = f(doc, self.caret, Instant::now());
        self.changed(old_lines, cx);
    }

    #[allow(dead_code)] // Used from Task 4 on.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        if let Some(caret) = doc.undo() {
            self.caret = caret;
            self.changed(old_lines, cx);
        }
    }

    #[allow(dead_code)] // Used from Task 4 on.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_mut() else { return };
        let old_lines = doc.shared_lines();
        if let Some(caret) = doc.redo() {
            self.caret = caret;
            self.changed(old_lines, cx);
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
        // This needs the old text, so the edit above copied the lines once: the price of
        // comparing blocks by their text.
        let new_blocks = md_blocks::split(&new_lines);
        let (range, count) = md_blocks::changed_blocks(&self.blocks, &old_lines, &new_blocks, &new_lines);
        let tail = range.start + count;
        self.blocks.splice(range.clone(), new_blocks[range.start..tail].iter().cloned());
        // The blocks after the change kept their kind and text but may have moved.
        for (block, new) in self.blocks[tail..].iter_mut().zip(&new_blocks[tail..]) {
            block.lines = new.lines.clone();
        }
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

    #[allow(dead_code)] // Used from Task 4 on.
    pub fn set_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
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

    /// The conflict's two buttons: keep mine writes the document over the file at once, adopting
    /// the file's fingerprint so the save's check passes; reload drops the edits.
    #[allow(dead_code)] // Used from Task 4 on.
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
///
/// A markdown file is never highlighted and has no size limit but `MAX_PLAIN_BYTES`: clean UTF-8
/// with one kind of line ending becomes a document whose lines (tabs kept, the empty last line
/// after a final break kept) are the content's; anything else is plain text, read-only, saying why.
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
        Ok(Capped::Bytes(bytes)) if is_markdown(path) => match markdown_document(&bytes) {
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
    Some(Loaded { content, fingerprint, plain: why_plain, diff, doc, read_only })
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
    #[allow(dead_code)] // Used from Task 4 on.
    pub fn active_viewer_tab(&self) -> Option<TabId> {
        let p = self.tree.pane(self.tree.focused)?;
        let id = *p.ids.get(p.active)?;
        (matches!(p.tabs.get(p.active)?, Tab::Viewer { .. }) && self.viewers.contains_key(&id)).then_some(id)
    }

    /// Writes every viewer's unsaved edits now, on this thread, for a quit; returns the errors
    /// (conflicts and failed writes), whose edits are not on disk.
    pub fn save_dirty_viewers_now(&mut self, cx: &mut App) -> Vec<String> {
        let viewers: Vec<Entity<Viewer>> = self.viewers.values().cloned().collect();
        viewers
            .into_iter()
            .filter_map(|v| {
                v.update(cx, |v, cx| {
                    let saved = v.save_now();
                    if saved.is_err() {
                        cx.notify();
                    }
                    // The conflict's text does not name the file; a write error's does.
                    saved.err().map(|e| if e == CONFLICT_NOTICE { format!("{}: {e}", v.path.display()) } else { e })
                })
            })
            .collect()
    }

    /// Flips a markdown tab between rendered and raw, in the layout (saved) and in its viewer.
    #[allow(dead_code)] // Used from Task 4 on.
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
        let rs = dir.path().join("x.rs"); std::fs::write(&rs, "fn main() {}\n").unwrap();
        assert!(read_file(&rs, None, None, 1 << 20, false).unwrap().doc.is_none(), "not markdown: no document");
    }
}
