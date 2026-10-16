//! The rendered markdown view (spec 7): one row of a virtualised gpui `list` per top-level block,
//! each block drawn from its inline runs (`md_blocks::inline`), cached per block in the viewer
//! and dropped only for the blocks an edit changed. Paragraphs are in the UI font, code in the
//! terminal's, and a table is a grid of cells at the paragraph size (its source in the terminal
//! font when it does not parse); a task item's box toggles the item through an undoable edit, and
//! a link opens on Cmd+click. An image is drawn from disk, or from the cache once a web
//! address's fetch has landed (`remote_image`), several in one paragraph in a row; Cmd+click
//! follows the link around it, or else shows it in Preview. An image whose path is a local video
//! is its Quick Look poster with a play glyph, and local audio a box with its name and size
//! (`media`): Cmd+click plays them. Remote video and audio, and a `webm`, are links. A `mermaid`
//! code block is its diagram, rendered by headless Chrome into a cache (`mermaid`); until the
//! PNG is there, or when it cannot be made, the block is its source with a line saying why.
//!
//! The block holding the caret (and every block a selection touches) is drawn raw instead, by
//! `RawBlockElement`: its source lines in the terminal font, wrapped to the view's width, with the
//! caret, the selection and the marked text. A click on a rendered block puts the caret at the
//! source position under it, Up and Down cross from block to block (`navigate`), and the input
//! handler sits on a canvas over the whole list, so typing reaches the document even when the
//! caret's block is scrolled out of view.

use crate::config::WideTables;
use crate::edit::{self, Pos};
use crate::highlight::Highlighter;
use crate::md_blocks::{self, Block, BlockKind, Inline, Run};
use crate::md_table::{self, Align};
use crate::media::{self, Media, Poster};
use crate::mermaid::{self, Diagram, MermaidTheme};
use crate::palette::Rgb;
use crate::remote_image::{self, Remote};
use crate::theme::Theme;
use crate::viewer::{self, Hit, Span, Viewer, ViewerEvent};
use crate::viewer_element::{self as element, RawColors, RawLayout};
use gpui::{
    canvas, div, font, img, list, point, prelude::*, px, relative, size, AnyElement, App, AvailableSpace, Bounds, Context, CursorStyle, DispatchPhase, Div, Element, ElementId, ElementInputHandler,
    Entity, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, InteractiveText, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, Point, ScrollHandle, SharedString, StrikethroughStyle, Style, StyledText, TextAlign, TextLayout, TextRun, UnderlineStyle, Window,
};
use std::cell::Cell;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// The paragraph size as a multiple of the font size.
pub const PARA: f32 = 1.1;
/// The heading sizes, levels one to six, as multiples of the paragraph size.
pub const HEADING: [f32; 6] = [1.9, 1.5, 1.25, 1.1, 1.0, 1.0];
/// The line height of paragraphs, headings, quotes and items, as a multiple of their size.
const PARA_LINE: f32 = 1.5;
/// Space below every block, and above a heading.
const BLOCK_GAP: f32 = 6.0;
const HEADING_GAP: f32 = 8.0;
/// The height of a `Blank` block (blank lines before the first block).
const BLANK_H: f32 = 12.0;
/// Space above and below a rule.
const RULE_GAP: f32 = 8.0;
/// Padding inside a code, table or HTML block.
const CODE_PAD: f32 = 8.0;
const QUOTE_BAR: f32 = 3.0;
const CHECKBOX: f32 = 12.0;
/// The space on either side of the text.
const SIDE: f32 = 20.0;
/// The width of one character in the item marker column, as a multiple of the paragraph size:
/// an approximation of the UI font's average advance.
const CH_EM: f32 = 0.6;
/// A table cell's padding, across and down.
const CELL_PAD_X: f32 = 10.0;
const CELL_PAD_Y: f32 = 6.0;
/// The most of the view's width one table column takes.
const COLUMN_CAP: f32 = 0.45;
/// The play glyph over a poster: a disc of this diameter around a triangle in a box of this side,
/// painted together.
const PLAY_DISC: f32 = 40.0;
const PLAY_TRIANGLE: f32 = 16.0;
/// The rows a table's natural column widths are measured over; later rows take the widths found.
const MEASURED_ROWS: usize = 200;
/// A table with more body rows than this is drawn as its source, not as a grid.
const GRID_MAX_ROWS: usize = 500;

/// The size of a block's text as a multiple of the font size: paragraphs, lists, quotes and table
/// grids at `PARA`, headings at `PARA` times their level's step, code, HTML and front matter at
/// the terminal's size (a table drawn as its source sets the terminal's size itself).
pub fn scale(kind: &BlockKind) -> f32 {
    match kind {
        BlockKind::Heading(level) => heading_scale(*level),
        BlockKind::CodeBlock { .. } | BlockKind::Html | BlockKind::Metadata => 1.0,
        BlockKind::Paragraph | BlockKind::List | BlockKind::Quote | BlockKind::Table | BlockKind::Rule | BlockKind::Blank => PARA,
    }
}

fn heading_scale(level: u8) -> f32 {
    PARA * HEADING[usize::from(level.clamp(1, 6) - 1)]
}

/// Where an image (or a linked file) is on disk: a relative path against the markdown file's
/// directory, with `.` and `..` resolved lexically; an absolute path as it is. A web address has
/// no path.
pub fn resolve_image(base_dir: &Path, path: &str) -> Option<PathBuf> {
    if remote_image::is_remote(path) {
        return None;
    }
    let mut out = PathBuf::new();
    for c in base_dir.join(path).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// One block's rendering, cached in `Viewer.block_cache`: its inline content (for a table its
/// cells and measured column widths), and for a code block the colours of its lines with the
/// highlighter that made them (a theme change makes a new highlighter, and the colours are made
/// again) and the text runs drawn from them.
pub struct Rendered {
    pieces: Vec<Piece>,
    pub code_spans: Option<(Arc<Highlighter>, Vec<Vec<Span>>)>,
    code_runs: Option<CodeRuns>,
}

/// One inline as a row draws it: its runs and text behind `Rc` or in a `SharedString`, so a
/// repaint clones handles, not strings.
enum Piece {
    Para(Rc<[Run]>),
    Heading(u8, Rc<[Run]>),
    Item { depth: u8, marker: String, task: Option<bool>, runs: Rc<[Run]> },
    /// A code block: its text as parsed (for mapping a press back to the source) and as drawn
    /// (tabs expanded, the trailing newline dropped).
    Code { info: String, text: Rc<str>, drawn: SharedString },
    /// A code block whose info string is `mermaid`: a diagram.
    Diagram(DiagramPiece),
    Quote(Rc<[Run]>),
    Rule,
    Table(TablePiece),
    Html(SharedString),
    Metadata(SharedString),
    /// A paragraph that is one image.
    Image(ImagePiece),
    /// A paragraph of images only (a row of badges).
    Images(Vec<ImagePiece>),
    /// An image whose path is video or audio, or media drawn as a link.
    Media(MediaPiece),
}

impl Piece {
    /// The paths of the images the piece shows.
    fn image_paths(&self) -> impl Iterator<Item = &String> {
        let images = match self {
            Piece::Image(image) => std::slice::from_ref(image),
            Piece::Images(images) => images.as_slice(),
            _ => &[],
        };
        images.iter().map(|i| &i.path)
    }
}

/// One image as a row draws it: its alt text (or its path when it has none) for the box shown
/// when it cannot be drawn, its path, and the target of the link around it, if any.
struct ImagePiece {
    label: SharedString,
    path: String,
    link: Option<String>,
}

impl ImagePiece {
    fn new(alt: String, path: String, link: Option<String>) -> ImagePiece {
        let label = if alt.is_empty() { path.clone().into() } else { alt.into() };
        ImagePiece { label, path, link }
    }
}

/// A diagram block as a row draws it: its source as parsed and as drawn (as a code block's, for
/// the states that show it), and its cache key with the theme colours it was made for (made in
/// `fill`, again when the theme changed).
struct DiagramPiece {
    text: Rc<str>,
    drawn: SharedString,
    key: Option<(MermaidTheme, String)>,
}

/// One video, audio file or media link as a row draws it: its label (as an image's), its path, the
/// target of the link around it, and what it is drawn as (`None`: a link, for a web address or a
/// `webm`).
struct MediaPiece {
    label: SharedString,
    path: String,
    link: Option<String>,
    kind: Option<Media>,
}

impl MediaPiece {
    /// The media an image path names, or `None` when it is an image to draw.
    fn of(image: &ImagePiece) -> Option<MediaPiece> {
        let kind = media::kind(&image.path);
        (kind.is_some() || media::is_link(&image.path)).then(|| MediaPiece { label: image.label.clone(), path: image.path.clone(), link: image.link.clone(), kind })
    }
}

/// Adds the pieces `inline` is drawn as to `out`: one, except that an image whose path is media
/// is drawn as media, and a row of images is split around the media in it (each on its own line,
/// the images on either side still in rows).
fn push_pieces(inline: Inline, out: &mut Vec<Piece>) {
    let images = match inline {
        Inline::Image { alt, path, link } => vec![ImagePiece::new(alt, path, link)],
        Inline::Images(images) => images.into_iter().map(|i| ImagePiece::new(i.alt, i.path, i.link)).collect(),
        other => return out.push(Piece::from(other)),
    };
    let mut row: Vec<ImagePiece> = Vec::new();
    let flush = |row: &mut Vec<ImagePiece>, out: &mut Vec<Piece>| match row.len() {
        0 => {}
        1 => out.extend(row.pop().map(Piece::Image)),
        _ => out.push(Piece::Images(std::mem::take(row))),
    };
    for image in images {
        match MediaPiece::of(&image) {
            Some(m) => {
                flush(&mut row, out);
                out.push(Piece::Media(m));
            }
            None => row.push(image),
        }
    }
    flush(&mut row, out);
}

impl From<Inline> for Piece {
    fn from(inline: Inline) -> Piece {
        match inline {
            Inline::Para(runs) => Piece::Para(runs.into()),
            Inline::Heading(level, runs) => Piece::Heading(level, runs.into()),
            Inline::Item { depth, marker, task, runs } => Piece::Item { depth, marker, task, runs: runs.into() },
            Inline::Code { text, info } if mermaid::is_diagram_info(&info) => Piece::Diagram(DiagramPiece { drawn: code_lines(&text).join("\n").into(), text: text.into(), key: None }),
            Inline::Code { info, text } => Piece::Code { info, drawn: code_lines(&text).join("\n").into(), text: text.into() },
            Inline::Quote(runs) => Piece::Quote(runs.into()),
            Inline::Rule => Piece::Rule,
            Inline::Table(t) => Piece::Table(TablePiece::new(t)),
            Inline::Html(t) => Piece::Html(t.into()),
            Inline::Metadata(t) => Piece::Metadata(t.into()),
            Inline::Image { alt, path, link } => Piece::Image(ImagePiece::new(alt, path, link)),
            Inline::Images(images) => Piece::Images(images.into_iter().map(|i| ImagePiece::new(i.alt, i.path, i.link)).collect()),
        }
    }
}

/// A table block as the view draws it: a grid of its cells when it parses and has at most
/// `GRID_MAX_ROWS` body rows, else its source in the terminal font, as before tables were drawn.
enum TablePiece {
    Grid(Grid, Widths),
    Source(SharedString),
}

impl TablePiece {
    fn new(source: String) -> TablePiece {
        match md_table::parse(&source) {
            Some(t) if t.rows.len() <= GRID_MAX_ROWS => {
                let rows = std::iter::once(t.header).chain(t.rows).map(|row| row.into_iter().map(GridCell::new).collect()).collect();
                TablePiece::Grid(Grid { align: t.align, rows }, Widths { key: None, natural: Vec::new(), scroll: ScrollHandle::new() })
            }
            _ => TablePiece::Source(source.into()),
        }
    }
}

/// A parsed table's cells, the header first, every row as long as `align`.
struct Grid {
    align: Vec<Align>,
    rows: Vec<Vec<GridCell>>,
}

/// One cell: its source text (trimmed, `\|` kept) and its inline runs, whose source ranges are
/// bytes of that text.
struct GridCell {
    text: Rc<str>,
    runs: Rc<[Run]>,
}

impl GridCell {
    /// The cell's runs as a paragraph's: emphasis, code and links render, `\|` is drawn as `|`
    /// (inside a code span too) and an image shows its alt text (a row of them their alt texts).
    /// Text that would parse as another block (a list marker, a heading's hashes) is drawn as it
    /// is.
    fn new(text: String) -> GridCell {
        let plain = |shown: String| vec![Run { text: shown, style: md_blocks::Style::default(), source: 0..text.len() }];
        let mut runs = match md_blocks::inline(&text, &BlockKind::Paragraph).as_slice() {
            [] => Vec::new(),
            [Inline::Para(runs)] => runs.clone(),
            [Inline::Image { alt, .. }] => plain(alt.clone()),
            [Inline::Images(images)] => plain(images.iter().map(|i| i.alt.as_str()).collect::<Vec<_>>().join(" ")),
            _ => plain(text.replace("\\|", "|")),
        };
        for run in runs.iter_mut().filter(|r| r.style.code) {
            run.text = run.text.replace("\\|", "|");
        }
        GridCell { runs: runs.into(), text: text.into() }
    }
}

/// A table's measured widths: each column's natural width (its widest cell over the first
/// `MEASURED_ROWS` rows, padding and hairline included) for the prose family, the terminal family and the size in `key`,
/// and the sideways scroll of a table in `WideTables::Scroll`. Measured once per block, again when
/// the family or size changed; an edit makes a new entry, which is fine.
struct Widths {
    key: Option<(SharedString, SharedString, f32)>,
    natural: Vec<f32>,
    scroll: ScrollHandle,
}

/// The columns' drawn widths from their natural widths in a view `view` wide: each at most
/// `COLUMN_CAP` of the view; in `Wrap`, columns summing to more than the view shrink in proportion
/// to fit it (their cells wrap); in `Scroll` they keep the capped widths and the table scrolls.
pub fn column_widths(natural: &[f32], view: f32, mode: WideTables) -> Vec<f32> {
    let capped: Vec<f32> = natural.iter().map(|w| w.min(view * COLUMN_CAP)).collect();
    let sum: f32 = capped.iter().sum();
    match mode {
        WideTables::Wrap if sum > view && sum > 0.0 => capped.iter().map(|w| w * view / sum).collect(),
        _ => capped,
    }
}

/// Measures block `ix`'s table, when it is a grid not yet measured at the frame's families and size:
/// each column's widest cell over the first `MEASURED_ROWS` rows (the header semibold), its text's
/// width plus the cell's padding and its 1 px hairline. The list's row closure calls it, the one
/// place with a `Window` to measure text with.
fn measure_table(v: &mut Viewer, ix: usize, ctx: &BlockCtx, window: &mut Window) {
    let Some(Some(r)) = v.block_cache.get_mut(ix) else { return };
    let Some(Piece::Table(TablePiece::Grid(grid, widths))) = r.pieces.first_mut() else { return };
    let size = ctx.font_size * PARA;
    // A code span in a cell is measured in the terminal family, so it is in the key too.
    if widths.key.as_ref().is_some_and(|(ui, mono, at)| *ui == ctx.ui && *mono == ctx.mono && *at == size) {
        return;
    }
    let mut natural = vec![0.0f32; grid.align.len()];
    for (i, row) in grid.rows.iter().take(MEASURED_ROWS).enumerate() {
        let weight = if i == 0 { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
        for (w, cell) in natural.iter_mut().zip(row) {
            let (text, runs, _) = styled_runs(&cell.runs, weight, ctx.text, ctx);
            let text_w = if text.is_empty() { 0.0 } else { f32::from(window.text_system().layout_line(&text, px(size), &runs, None).width) };
            *w = w.max(text_w + CELL_PAD_X * 2.0 + 1.0);
        }
    }
    widths.key = Some((ctx.ui.clone(), ctx.mono.clone(), size));
    widths.natural = natural;
}

/// A code block's text runs, with what they were made from: whether the highlighter's colours
/// were there, the terminal font and the text colour. Any of them changing makes them again.
struct CodeRuns {
    coloured: bool,
    mono: SharedString,
    text: Hsla,
    runs: Rc<Vec<TextRun>>,
}

/// The most lines a code block may have and still be coloured: the highlighter runs on the UI
/// thread when the block is first drawn.
pub const HIGHLIGHT_MAX_LINES: usize = 2000;

/// Whether a code block of `bytes` bytes and `lines` lines is coloured: at most the configured
/// highlight limit (`limit`, `viewer.highlight_limit_bytes`) and `HIGHLIGHT_MAX_LINES`. A bigger
/// one is drawn plain in the terminal font.
pub fn should_highlight(bytes: usize, lines: usize, limit: usize) -> bool {
    bytes <= limit && lines <= HIGHLIGHT_MAX_LINES
}

/// A code block's lines as drawn: the trailing newline dropped and tabs expanded.
fn code_lines(text: &str) -> Vec<String> {
    text.strip_suffix('\n').unwrap_or(text).split('\n').map(|l| l.replace('\t', "    ")).collect()
}

/// Fills block `ix`'s cache entry (`fill_cache`), moves its web images along (`track_remote`),
/// its videos' posters (`track_media`) and its diagram (`track_diagram`). The list calls it for
/// each rendered row it draws, so a block shown raw (the one being typed in) fetches, makes and
/// renders nothing.
pub fn fill(v: &mut Viewer, ix: usize, highlighter: Option<&Arc<Highlighter>>, ctx: &BlockCtx, cx: &mut Context<Viewer>) {
    fill_cache(v, ix, highlighter, ctx);
    track_remote(v, ix, cx);
    track_media(v, ix, &ctx.base_dir, cx);
    track_diagram(v, ix, &ctx.mermaid, cx);
}

/// Block `ix`'s diagram: its cache key made for the frame's colours (again when they changed),
/// then its state moved along once (`mermaid::advance`): one not seen yet is ready from the cache,
/// not to be rendered without Chrome, or queued; one whose render ended is ready or failed. While
/// it is still rendering, the render that has the turn starts (`Viewer::start_renders`); its end
/// draws the window again, which brings each viewer showing the diagram here.
fn track_diagram(v: &mut Viewer, ix: usize, theme: &MermaidTheme, cx: &mut Context<Viewer>) {
    let Some(Some(r)) = v.block_cache.get_mut(ix) else { return };
    let Some(Piece::Diagram(d)) = r.pieces.first_mut() else { return };
    if !d.key.as_ref().is_some_and(|(made_for, _)| made_for == theme) {
        d.key = Some((theme.clone(), mermaid::cache_key(&d.text, theme, mermaid::SCALE)));
    }
    let (Some((_, key)), text) = (d.key.as_ref(), &d.text) else { return };
    let state = v.diagrams.get(key);
    if matches!(state, Some(Diagram::Ready { .. } | Diagram::Error(_) | Diagram::NoChrome)) {
        return;
    }
    let next = mermaid::advance(state, key, || mermaid::page(text, theme), mermaid::cache_dir, || mermaid::chrome().is_some());
    let rendering = next.as_ref().is_none_or(|n| *n == Diagram::Rendering);
    if let Some(next) = next {
        v.diagrams.insert(key.clone(), next);
    }
    if rendering {
        Viewer::start_renders(cx);
    }
}

/// Block `ix`'s local videos and audio: each video's poster moved along once (`media::advance`),
/// which reads the file's mtime; each audio file's size read the first time it is shown. While
/// any poster is still being made, the job that has the turn starts (`Viewer::start_thumbnails`);
/// its end draws the window again, which brings each viewer showing the video here.
fn track_media(v: &mut Viewer, ix: usize, base_dir: &Path, cx: &mut Context<Viewer>) {
    let Some(Some(r)) = v.block_cache.get(ix) else { return };
    let mut dir = None;
    let mut making = false;
    for m in r.pieces.iter().filter_map(|p| if let Piece::Media(m) = p { Some(m) } else { None }) {
        let Some(file) = resolve_image(base_dir, &m.path) else { continue };
        match m.kind {
            Some(Media::Video) => {
                let state = v.posters.get(&file);
                if matches!(state, Some(Poster::Ready(_) | Poster::Failed)) {
                    continue;
                }
                let next = media::advance(state, &file, || dir.get_or_insert_with(media::poster_dir).clone());
                making |= next.as_ref().is_none_or(|n| *n == Poster::Making);
                if let Some(next) = next {
                    v.posters.insert(file, next);
                }
            }
            Some(Media::Audio) => {
                v.sizes.entry(file).or_insert_with_key(|file| std::fs::metadata(file).ok().map(|m| m.len()));
            }
            None => {}
        }
    }
    if making {
        Viewer::start_thumbnails(cx);
    }
}

/// Block `ix`'s web images, each moved along once (`remote_image::advance`): one not seen yet is
/// ready from the cache or queued, one whose fetch ended is ready or failed. While any is still
/// fetching, the fetches that now have a slot start (`Viewer::start_fetches`); a fetch's end
/// draws the window again, which brings each viewer showing the image here.
fn track_remote(v: &mut Viewer, ix: usize, cx: &mut Context<Viewer>) {
    let Some(Some(r)) = v.block_cache.get(ix) else { return };
    let mut dir = None;
    let mut fetching = false;
    for url in r.pieces.iter().flat_map(Piece::image_paths).filter(|p| remote_image::is_remote(p)) {
        let state = v.remote.get(url);
        if matches!(state, Some(Remote::Ready(_) | Remote::Failed)) {
            continue;
        }
        let next = remote_image::advance(state, url, || dir.get_or_insert_with(remote_image::cache_dir).clone());
        fetching |= next.as_ref().is_none_or(|n| *n == Remote::Fetching);
        if let Some(next) = next {
            v.remote.insert(url.clone(), next);
        }
    }
    if fetching {
        Viewer::start_fetches(cx);
    }
}

/// Fills block `ix`'s cache entry if it is empty, and colours a code block once the highlighter is
/// there (again when it changed), unless it is over the highlight limit (`ctx.highlight_limit`)
/// or `HIGHLIGHT_MAX_LINES` (`should_highlight`); a code block's runs are made again only when
/// its colours, the font or the text colour changed. Nothing is computed for a block already
/// cached.
fn fill_cache(v: &mut Viewer, ix: usize, highlighter: Option<&Arc<Highlighter>>, ctx: &BlockCtx) {
    let (Some(doc), Some(block), Some(slot)) = (v.doc.as_ref(), v.blocks.get(ix), v.block_cache.get_mut(ix)) else { return };
    let r = slot.get_or_insert_with(|| {
        let lines = doc.lines();
        let end = block.lines.end.min(lines.len());
        let text = lines[block.lines.start.min(end)..end].join("\n");
        let mut pieces = Vec::new();
        for inline in md_blocks::inline(&text, &block.kind) {
            push_pieces(inline, &mut pieces);
        }
        Rendered { pieces, code_spans: None, code_runs: None }
    });
    let Some(Piece::Code { info, text, .. }) = r.pieces.first() else { return };
    if let Some(h) = highlighter {
        if !r.code_spans.as_ref().is_some_and(|(by, _)| Arc::ptr_eq(by, h)) && should_highlight(text.len(), text.lines().count(), ctx.highlight_limit) {
            r.code_spans = Some((h.clone(), h.highlight_token(info, &code_lines(text))));
            // New colours (a theme change made a new highlighter): the runs drawn from the old
            // ones go too.
            r.code_runs = None;
        }
    }
    let coloured = r.code_spans.is_some();
    if !r.code_runs.as_ref().is_some_and(|c| c.coloured == coloured && c.mono == ctx.mono && c.text == ctx.text) {
        let spans = r.code_spans.as_ref().map(|(_, s)| s.as_slice());
        r.code_runs = Some(CodeRuns { coloured, mono: ctx.mono.clone(), text: ctx.text, runs: Rc::new(code_runs(&code_lines(text), spans, ctx)) });
    }
}

/// What every block of one frame shares: the viewer (for the task boxes), the fonts, the sizes,
/// the colours and the directory images and links resolve against.
pub struct BlockCtx {
    pub viewer: Entity<Viewer>,
    pub ui: SharedString,
    pub mono: SharedString,
    pub font_size: f32,
    /// The terminal's line height, as a multiple of the font size, for code and tables.
    pub mono_line_height: f32,
    pub base_dir: PathBuf,
    /// A code block over this many bytes is drawn plain (`viewer.highlight_limit_bytes`).
    pub highlight_limit: usize,
    /// How a table wider than the view is drawn (`viewer.wide_tables`).
    pub wide_tables: WideTables,
    /// The width a block's content has: the list's width at the last frame less the side space.
    /// Not positive before the list's first layout.
    pub view_w: f32,
    /// A table grid was drawn this frame, so the canvas checks the width it was drawn for.
    pub drew_grid: Rc<Cell<bool>>,
    pub text: Hsla,
    pub dim: Hsla,
    pub link: Hsla,
    pub code_bg: Hsla,
    pub border: Hsla,
    pub on_accent: Hsla,
    /// The colour of a diagram's error line (the tree's deleted colour).
    pub deleted: Hsla,
    /// The colours diagrams are drawn in, from the chrome.
    pub mermaid: MermaidTheme,
    /// The raw blocks' colours: the raw view's markdown colours, the selection (the find match
    /// colour, as in the raw view), the current find hit and the caret.
    pub raw: RawColors,
    pub selection: Rgb,
    pub find_current: Rgb,
    pub caret: Rgb,
}

/// The rendered view of a document: a `list` with one row per block, each built from the block's
/// cached rendering (filled on demand) or drawn raw when the caret or the selection is in it, and
/// a tail row below the last block. The list scrolls itself and keeps its place in
/// `Viewer.list_state`. A canvas over the list takes the text input.
/// A code block over `highlight_limit` bytes (`viewer.highlight_limit_bytes`) is drawn plain, and a
/// table wider than the view wraps or scrolls by `wide_tables` (`viewer.wide_tables`).
#[allow(clippy::too_many_arguments)]
pub fn render(viewer: &Entity<Viewer>, theme: &Theme, ui_family: SharedString, mono_family: SharedString, font_size: f32, highlighter: Option<Arc<Highlighter>>, highlight_limit: usize, wide_tables: WideTables, cx: &mut App) -> AnyElement {
    // The raw blocks of this frame record their layouts as they are laid out.
    viewer.update(cx, |v, _| v.raw_blocks.clear());
    let v = viewer.read(cx);
    let state = v.list_state.clone();
    // Read here, not in the row closure: the list holds its state while it lays the rows out.
    let view_w = f32::from(v.list_state.viewport_bounds().size.width) - 2.0 * SIDE;
    let drew_grid = Rc::new(Cell::new(false));
    let ctx = Rc::new(BlockCtx {
        viewer: viewer.clone(),
        ui: ui_family,
        mono: mono_family,
        font_size,
        mono_line_height: v.line_height,
        base_dir: v.path.parent().map(Path::to_path_buf).unwrap_or_default(),
        highlight_limit,
        wide_tables,
        view_w,
        drew_grid: drew_grid.clone(),
        text: theme.chrome.text.hsla(),
        dim: theme.chrome.text_dim.hsla(),
        link: theme.chrome.focus_ring.hsla(),
        code_bg: theme.chrome.tab_inactive.hsla(),
        border: theme.chrome.border.hsla(),
        on_accent: theme.chrome.window_bg.hsla(),
        deleted: theme.tree.deleted.hsla(),
        mermaid: MermaidTheme::from_chrome(&theme.chrome),
        raw: RawColors { text: theme.chrome.text, dim: theme.chrome.text_dim, code: theme.tree.modified, link: theme.chrome.focus_ring },
        selection: theme.semantic.find_match,
        find_current: theme.semantic.find_current,
        caret: theme.chrome.text,
    });
    let input = input_canvas(viewer.clone(), view_w, drew_grid);
    div()
        .id(("md-view", viewer.entity_id()))
        .relative()
        .size_full()
        .child(
            list(state, move |ix, window, cx| {
                // The caret's block is raw only while the viewer has the focus: gpui redraws the
                // window when the focus moves, so a blur turns it back to rendered.
                let (count, raw, tail_caret) = {
                    let v = ctx.viewer.read(cx);
                    let focused = v.focus.is_focused(window);
                    (v.blocks.len(), v.renders_raw(ix, focused), focused && v.pending_append)
                };
                if ix >= count {
                    return tail_element(&ctx, tail_caret);
                }
                if raw {
                    return raw_row(ix, &ctx, window, cx);
                }
                // `render_item` gets the App, not the viewer: the cache is filled through the
                // entity, then read while the row is built.
                ctx.viewer.update(cx, |v, cx| {
                    fill(v, ix, highlighter.as_ref(), &ctx, cx);
                    measure_table(v, ix, &ctx, window);
                });
                let v = ctx.viewer.read(cx);
                match (v.blocks.get(ix), v.block_cache.get(ix).and_then(Option::as_ref)) {
                    (Some(block), Some(r)) => block_element(ix, &block.kind, r, v, &ctx),
                    _ => div().into_any_element(),
                }
            })
            .size_full(),
        )
        .child(input)
        .into_any_element()
}

/// The canvas over the list that registers the document's input handler every frame, whatever
/// rows are on screen, so typing reaches the active block even when it is scrolled away. It also
/// ends a drag selection wherever the button comes up. It paints nothing and takes no mouse
/// presses, so the rows below it get them. Its bounds are the list's: when a table grid was drawn
/// for a width (`view_w`, the last frame's) the list no longer has, as on the first frame or in a
/// resize, it asks for another frame, so the columns follow the view.
fn input_canvas(viewer: Entity<Viewer>, view_w: f32, drew_grid: Rc<Cell<bool>>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, cx| {
            if drew_grid.get() && (f32::from(bounds.size.width) - 2.0 * SIDE - view_w).abs() > 0.5 {
                window.request_animation_frame();
            }
            let focus = viewer.read(cx).focus.clone();
            window.handle_input(&focus, ElementInputHandler::new(bounds, viewer.clone()), cx);
            let v = viewer.clone();
            // In the capture phase, which no handler can stop, so a drag always ends.
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Capture && ev.button == MouseButton::Left {
                    v.update(cx, |v, _| v.mouse_up());
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The row below the last block, two paragraph lines high: a press there puts the caret in a new
/// paragraph at the end of the document (`Viewer::append_paragraph`), drawn here at the row's top
/// (`caret`) until the first edit writes the paragraph's blank lines.
fn tail_element(ctx: &BlockCtx, caret: bool) -> AnyElement {
    let viewer = ctx.viewer.clone();
    let line_h = ctx.font_size * PARA * PARA_LINE;
    div()
        .id("md-tail")
        .relative()
        .w_full()
        .h(px(line_h * 2.0))
        .cursor_text()
        .when(caret, |d| d.child(div().absolute().top_0().left(px(SIDE)).w(px(2.0)).h(px(line_h)).bg(ctx.caret.hsla())))
        .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _window, cx| viewer.update(cx, |v, cx| v.append_paragraph(cx)))
        .into_any_element()
}

/// A block drawn raw: its source lines (`Viewer::raw_lines`) in a `RawBlockElement`, with the
/// rendered rows' side padding and space below.
fn raw_row(ix: usize, ctx: &BlockCtx, window: &mut Window, cx: &mut App) -> AnyElement {
    let v = ctx.viewer.read(cx);
    let Some(doc) = v.doc.as_ref() else { return div().into_any_element() };
    let shown = v.raw_lines(ix);
    let (hits, current) = match &v.find {
        Some(f) => (f.hits.iter().filter(|h| shown.contains(&h.line)).copied().collect(), f.hits.get(f.current).copied()),
        None => (Vec::new(), None),
    };
    let el = RawBlockElement {
        viewer: ctx.viewer.clone(),
        lines: doc.shared_lines(),
        shown,
        family: ctx.mono.clone(),
        font_size: ctx.font_size,
        line_h: ctx.font_size * ctx.mono_line_height,
        colors: ctx.raw,
        caret: v.caret,
        marked: v.marked.clone(),
        focused: v.focus.is_focused(window),
        hits,
        current,
        selection: ctx.selection,
        find_current: ctx.find_current,
        caret_color: ctx.caret,
    };
    div().id(("md-block", ix)).w_full().px(px(SIDE)).pb(px(BLOCK_GAP)).child(el).into_any_element()
}

/// What a press on a rendered block maps through: each text element's layout with the source it
/// shows, and whether a task box took the press (so the caret stays put and the box's click
/// lands).
struct Parts {
    texts: Vec<(TextLayout, Part)>,
    task_press: Rc<Cell<bool>>,
}

/// The source behind one text element: inline runs, or a code block's text.
enum Part {
    Runs(Rc<[Run]>),
    Code(Rc<str>),
}

/// One block's row: its inlines in the block's type, 6 px of space below, with the frame's state
/// from `v` (the web images' fetches). A press puts the caret at the source position under it
/// (Shift extends the selection); a drag that reaches the row moves the selection's head there.
pub fn block_element(ix: usize, kind: &BlockKind, r: &Rendered, v: &Viewer, ctx: &BlockCtx) -> AnyElement {
    let size = ctx.font_size * scale(kind);
    let row = div()
        .id(("md-block", ix))
        .w_full()
        .px(px(SIDE))
        .pb(px(BLOCK_GAP))
        .text_color(ctx.text)
        .font_family(ctx.ui.clone())
        .text_size(px(size))
        .line_height(relative(PARA_LINE));
    let mut parts = Parts { texts: Vec::new(), task_press: Rc::new(Cell::new(false)) };
    let mut children: Vec<AnyElement> = Vec::with_capacity(r.pieces.len().max(1));
    if *kind == BlockKind::Blank {
        children.push(div().h(px(BLANK_H)).into_any_element());
    }
    for (j, piece) in r.pieces.iter().enumerate() {
        let child = match piece {
            Piece::Para(runs) => text_element(j, runs, FontWeight::NORMAL, ctx.text, ctx, &mut parts),
            Piece::Heading(level, runs) => div()
                .pt(px(HEADING_GAP))
                .text_size(px(ctx.font_size * heading_scale(*level)))
                .child(text_element(j, runs, FontWeight::SEMIBOLD, ctx.text, ctx, &mut parts))
                .into_any_element(),
            Piece::Quote(runs) => div()
                .border_l(px(QUOTE_BAR))
                .border_color(ctx.border)
                .pl(px(10.0))
                .child(text_element(j, runs, FontWeight::NORMAL, ctx.dim, ctx, &mut parts))
                .into_any_element(),
            Piece::Item { depth, marker, task, runs } => item_element(ix, j, *depth, marker, *task, runs, size, ctx, &mut parts),
            Piece::Code { text, drawn, .. } => code_element(text, drawn, r.code_runs.as_ref().map(|c| &c.runs), None, ctx, &mut parts),
            Piece::Diagram(d) => diagram_element(d, v, ctx, &mut parts),
            Piece::Rule => div().my(px(RULE_GAP)).h(px(1.0)).w_full().bg(ctx.border).into_any_element(),
            Piece::Table(TablePiece::Grid(grid, widths)) => grid_element(ix, grid, widths, ctx, &parts.task_press),
            Piece::Table(TablePiece::Source(t)) => mono_box(t, ctx.text, Some(ctx.code_bg), ctx),
            // An HTML block is its source, dimmed (spec 5.2).
            Piece::Html(t) => mono_box(t, ctx.dim, Some(ctx.code_bg), ctx),
            Piece::Metadata(t) => mono_box(t, ctx.dim, None, ctx),
            Piece::Image(image) => image_element(image, v, ctx, &parts.task_press),
            Piece::Media(m) => media_element(m, v, ctx, &parts.task_press),
            // Badges sit side by side, wrapping onto more lines when the view is narrow.
            Piece::Images(images) => div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(4.0))
                .children(images.iter().map(|image| image_element(image, v, ctx, &parts.task_press)))
                .into_any_element(),
        };
        children.push(child);
    }
    let Parts { texts, task_press } = parts;
    let texts = Rc::new(texts);
    // A grid's cells place the caret themselves and give the row no text to map a drag through.
    let grid = r.pieces.iter().any(|p| matches!(p, Piece::Table(TablePiece::Grid(..))));
    let (v_down, v_move, texts_down) = (ctx.viewer.clone(), ctx.viewer.clone(), texts.clone());
    row.children(children)
        .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _window, cx| {
            // A task box's press is for the box, and Cmd+click is for a link or an image: the
            // block stays rendered so their clicks land.
            if task_press.replace(false) || ev.modifiers.platform {
                return;
            }
            v_down.update(cx, |v, cx| {
                if let Some(p) = caret_at(v, ix, &texts_down, ev.position) {
                    v.place_caret(p, ev.modifiers.shift, cx);
                }
            });
        })
        .on_mouse_move(move |ev: &MouseMoveEvent, _window, cx| {
            // Over a table grid a drag would map to the block's first character, so the jitter of
            // a press on a cell would select back to it: the head waits for the raw block instead.
            if ev.pressed_button != Some(MouseButton::Left) || grid {
                return;
            }
            v_move.update(cx, |v, cx| {
                if v.is_selecting() {
                    if let Some(p) = caret_at(v, ix, &texts, ev.position) {
                        v.drag_to(p, cx);
                    }
                }
            });
        })
        .into_any_element()
}

/// The source position under a window point in rendered block `ix`, read when the press comes:
/// through the text element nearest the point (`index_for_position` gives a byte of its rendered
/// string, `source_offset_at` the byte of the block's text, `pos_of_offset` the position). A
/// point outside every text element (the block's padding, a heading's space above, a code
/// block's padding, an item's marker column) goes to the element vertically nearest it, at the
/// end of its row nearest the pointer. A block with no text (an image, a rule) puts the caret at
/// its first character.
fn caret_at(v: &Viewer, ix: usize, texts: &[(TextLayout, Part)], at: Point<Pixels>) -> Option<Pos> {
    let (doc, block) = (v.doc.as_ref()?, v.blocks.get(ix)?);
    let lines = doc.lines();
    let gap = |layout: &TextLayout| {
        let b = layout.bounds();
        f32::from(if at.y < b.top() { b.top() - at.y } else if at.y > b.bottom() { at.y - b.bottom() } else { px(0.0) })
    };
    let Some((layout, part)) = texts.iter().min_by(|a, b| gap(&a.0).total_cmp(&gap(&b.0))) else {
        return Some(doc.clamp(Pos { line: block.lines.start, ch: 0 }));
    };
    let byte = index_near(layout, at);
    Some(match part {
        Part::Runs(runs) => md_blocks::pos_of_offset(block, lines, md_blocks::source_offset_at(runs, byte)),
        Part::Code(text) => code_pos(block, lines, text, byte),
    })
}

/// The byte of a text element's string nearest a window point: the point is moved inside the
/// text's bounds first, so a point in the padding beside it lands on the nearest row's end.
fn index_near(layout: &TextLayout, at: Point<Pixels>) -> usize {
    let b = layout.bounds();
    let inside = point(at.x.max(b.left()).min((b.right() - px(1.0)).max(b.left())), at.y.max(b.top()).min((b.bottom() - px(1.0)).max(b.top())));
    layout.index_for_position(inside).unwrap_or_else(|e| e)
}

/// A byte of a code block's drawn text (its lines with tabs expanded, joined) to a document
/// position: the drawn line is the source line as many lines below the opening fence (or the
/// block's first line, for an indented block), and the column counts back through the tabs and
/// past the indent the source line has before the code.
fn code_pos(block: &Block, lines: &[String], text: &str, byte: usize) -> Pos {
    let drawn = code_lines(text);
    let code: Vec<&str> = text.strip_suffix('\n').unwrap_or(text).split('\n').collect();
    let (mut row, mut rest) = (0, byte);
    while row + 1 < drawn.len() && rest > drawn[row].len() {
        rest -= drawn[row].len() + 1;
        row += 1;
    }
    let shown = &drawn[row];
    let col = shown[..shown.floor_char_boundary(rest.min(shown.len()))].chars().count();
    let ch = element::ch_for_col(code.get(row).copied().unwrap_or(""), col);
    let first = block.lines.start.min(lines.len().saturating_sub(1));
    let fenced = lines.get(first).is_some_and(|l| {
        let t = l.trim_start();
        t.starts_with("```") || t.starts_with("~~~")
    });
    let last = md_blocks::pos_of_offset(block, lines, usize::MAX).line;
    let line = (first + usize::from(fenced) + row).min(last);
    let source = &lines[line];
    let code_line = code.get(row).copied().unwrap_or("");
    let indent = source.strip_suffix(code_line).map_or(0, |pre| pre.chars().count());
    Pos { line, ch: (indent + ch).min(source.chars().count()) }
}

/// The runs of a paragraph, heading, quote or item as one string with a `TextRun` per run (their
/// lengths sum to the string's, as `with_runs` requires), and the links' byte ranges with their
/// targets.
fn styled_runs(runs: &[Run], weight: FontWeight, color: Hsla, ctx: &BlockCtx) -> (String, Vec<TextRun>, Vec<(std::ops::Range<usize>, String)>) {
    let mut text = String::new();
    let mut out = Vec::with_capacity(runs.len());
    let mut links = Vec::new();
    for run in runs.iter().filter(|r| !r.text.is_empty()) {
        let s = &run.style;
        let start = text.len();
        text.push_str(&run.text);
        let mut f = font(if s.code || s.html { ctx.mono.clone() } else { ctx.ui.clone() });
        f.weight = if s.bold { FontWeight::BOLD } else { weight };
        f.style = if s.italic { FontStyle::Italic } else { FontStyle::Normal };
        let fg = if s.link.is_some() {
            ctx.link
        } else if s.html {
            ctx.dim
        } else {
            color
        };
        out.push(TextRun {
            len: run.text.len(),
            font: f,
            color: fg,
            background_color: s.code.then_some(ctx.code_bg),
            underline: s.link.as_ref().map(|_| UnderlineStyle { thickness: px(1.0), color: Some(ctx.link), wavy: false }),
            strikethrough: s.strike.then_some(StrikethroughStyle { thickness: px(1.0), color: Some(fg) }),
        });
        if let Some(url) = &s.link {
            links.push((start..text.len(), url.clone()));
        }
    }
    (text, out, links)
}

/// A run list as text: a `StyledText`, wrapped to open its links on Cmd+click when it has any. Its
/// layout goes into `parts`, so a press on the row maps through it.
fn text_element(j: usize, runs: &Rc<[Run]>, weight: FontWeight, color: Hsla, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let (element, layout) = runs_text(j, runs, weight, color, ctx);
    parts.texts.push((layout, Part::Runs(runs.clone())));
    element
}

/// A run list as a `StyledText` (an `InteractiveText` with id `("md-text", j)` opening its links
/// on Cmd+click, when it has any) and the layout it will have.
fn runs_text(j: usize, runs: &[Run], weight: FontWeight, color: Hsla, ctx: &BlockCtx) -> (AnyElement, TextLayout) {
    let (text, text_runs, links) = styled_runs(runs, weight, color, ctx);
    let styled = StyledText::new(text).with_runs(text_runs);
    let layout = styled.layout().clone();
    if links.is_empty() {
        return (styled.into_any_element(), layout);
    }
    let (ranges, targets): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    let (base_dir, viewer) = (ctx.base_dir.clone(), ctx.viewer.clone());
    let element = InteractiveText::new(("md-text", j), styled)
        .on_click(ranges, move |i, window, cx| {
            // A plain click is for the text (the row put the caret there); Cmd+click follows the
            // link.
            if window.modifiers().platform {
                if let Some(target) = targets.get(i) {
                    follow_link(link_action(target, &base_dir), &viewer, cx);
                }
            }
        })
        .into_any_element();
    (element, layout)
}

/// What Cmd+click on a link does. Nothing on disk is ever opened by the system, which could run
/// a program: a file is only shown in Finder, and a markdown file opens in arkos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// A web page or a mail address (`http`, `https`, `mailto`): the system opens it.
    OpenUrl(String),
    /// A file or folder (a path or a `file:` URL): Finder shows it.
    Reveal(PathBuf),
    /// A markdown file: a viewer tab opens it.
    OpenMarkdown(PathBuf),
    /// A link to a heading in this file, an empty target, or a scheme arkos does not follow.
    Nothing,
}

/// Classifies a link's target, resolving a path against the file's directory `base_dir`
/// (percent escapes decoded, a `#fragment` dropped).
pub fn link_action(target: &str, base_dir: &Path) -> LinkAction {
    let target = target.trim();
    let local = |path: &str| {
        let path = percent_decode(path.split('#').next().unwrap_or(""));
        if path.is_empty() {
            return LinkAction::Nothing;
        }
        let Some(resolved) = resolve_image(base_dir, &path) else { return LinkAction::Nothing };
        if viewer::is_markdown(&resolved) {
            LinkAction::OpenMarkdown(resolved)
        } else {
            LinkAction::Reveal(resolved)
        }
    };
    match link_scheme(target) {
        Some(("http" | "https" | "mailto", _)) => LinkAction::OpenUrl(target.to_string()),
        Some(("file", rest)) => {
            // `file:///p`, `file://localhost/p` or `file:/p`: an absolute path, without a query.
            let rest = rest.strip_prefix("//").map_or(rest, |r| r.strip_prefix("localhost").unwrap_or(r));
            match rest.split('?').next().filter(|p| p.starts_with('/')) {
                Some(path) => local(path),
                None => LinkAction::Nothing,
            }
        }
        Some(_) => LinkAction::Nothing,
        None => local(target),
    }
}

/// A URL's scheme, lowercased, and what follows its colon: two or more letters, digits, `+`, `-`
/// or `.` starting with a letter. A path has none.
fn link_scheme(target: &str) -> Option<(&'static str, &str)> {
    let (scheme, rest) = target.split_once(':')?;
    let valid = scheme.len() >= 2 && scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return None;
    }
    let known = ["http", "https", "mailto", "file"].into_iter().find(|k| scheme.eq_ignore_ascii_case(k));
    Some((known.unwrap_or("other"), rest))
}

/// `%XX` escapes decoded, as a link to a file with a space in its name is written; text that does
/// not decode to UTF-8 is kept as it was.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Carries out a link's action: the URL through the system, a file shown in Finder (`open -R`, on
/// the background executor), a markdown file handed to `Root` to open in a tab.
fn follow_link(action: LinkAction, viewer: &Entity<Viewer>, cx: &mut App) {
    match action {
        LinkAction::OpenUrl(url) => cx.open_url(&url),
        LinkAction::Reveal(path) => cx
            .background_executor()
            .spawn(async move {
                let status = std::process::Command::new("/usr/bin/open").arg("-R").arg(&path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
                match status {
                    Ok(s) if s.success() => {}
                    Ok(s) => eprintln!("arkos: cannot show {} in Finder ({s})", path.display()),
                    Err(e) => eprintln!("arkos: cannot run /usr/bin/open: {e}"),
                }
            })
            .detach(),
        LinkAction::OpenMarkdown(path) => viewer.update(cx, |_, cx| cx.emit(ViewerEvent::OpenPath(path))),
        LinkAction::Nothing => {}
    }
}

/// A list item: the marker column (a bullet, or the number with its delimiter) indented by its
/// depth, a task's box, and the item's text.
#[allow(clippy::too_many_arguments)]
fn item_element(ix: usize, j: usize, depth: u8, marker: &str, task: Option<bool>, runs: &Rc<[Run]>, size: f32, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let ch = size * CH_EM;
    let bullet = match marker {
        "-" | "*" | "+" | "" => "\u{2022}".to_string(),
        numbered => numbered.to_string(),
    };
    let line_h = size * PARA_LINE;
    // The item's line is found when the box is clicked, from where its text starts in the block:
    // the block may have moved since this frame.
    let offset = runs.first().map(|r| r.source.start);
    let check = task.map(|done| {
        let viewer = ctx.viewer.clone();
        let press = parts.task_press.clone();
        div()
            .id(("md-task", j))
            .flex_none()
            .mt(px(((line_h - CHECKBOX) / 2.0).max(0.0)))
            .mr(px(ch * 0.75))
            .size(px(CHECKBOX))
            .rounded(px(3.0))
            .border_1()
            .border_color(if done { ctx.link } else { ctx.dim })
            .when(done, |d| d.bg(ctx.link).child("\u{2713}"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(CHECKBOX - 2.0))
            .line_height(px(CHECKBOX))
            .text_color(ctx.on_accent)
            .cursor_pointer()
            // The row's press handler runs next and leaves the caret alone for this press, so the
            // block stays rendered and the box's click lands.
            .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _window, _cx| press.set(true))
            .when_some(offset, move |d, offset| d.on_click(move |_ev, _window, cx| viewer.update(cx, |v, cx| toggle_item(v, ix, offset, cx))))
    });
    div()
        .flex()
        .flex_row()
        .items_start()
        .pl(px(f32::from(depth) * 2.0 * ch))
        .child(div().flex_none().min_w(px(2.0 * ch)).pr(px(ch * 0.5)).text_color(ctx.dim).child(bullet))
        .children(check)
        .child(div().flex_1().min_w_0().child(text_element(j, runs, FontWeight::NORMAL, ctx.text, ctx, parts)))
        .into_any_element()
}

/// Flips the task box of the item in block `ix` whose text starts at `offset` in the block's text:
/// an undoable edit of that one line, away from the caret, so the view does not move.
fn toggle_item(v: &mut Viewer, ix: usize, offset: usize, cx: &mut Context<Viewer>) {
    let (Some(doc), Some(block)) = (v.doc.as_ref(), v.blocks.get(ix)) else { return };
    let line = md_blocks::pos_of_offset(block, doc.lines(), offset).line;
    v.edit_elsewhere(
        |doc, caret, now| {
            let Some(flipped) = (line < doc.line_count()).then(|| md_blocks::toggle_task(&doc.lines()[line])).flatten() else { return caret };
            let end = doc.lines()[line].chars().count();
            doc.replace_range(edit::Pos { line, ch: 0 }, edit::Pos { line, ch: end }, &flipped, now);
            caret
        },
        cx,
    );
}

/// A code block: its lines in the terminal font on the inactive tab colour, coloured by the
/// highlighter's spans when they are there. `text` is the block's code as parsed, `drawn` as
/// shown, `runs` its cached runs (`fill` makes them; made here only when missing).
fn code_element(text: &Rc<str>, drawn: &SharedString, runs: Option<&Rc<Vec<TextRun>>>, info: Option<(SharedString, Hsla)>, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let runs = runs.map_or_else(|| code_runs(&code_lines(text), None, ctx), |r| r.as_ref().clone());
    let styled = StyledText::new(drawn.clone()).with_runs(runs);
    parts.texts.push((styled.layout().clone(), Part::Code(text.clone())));
    div()
        .bg(ctx.code_bg)
        .p(px(CODE_PAD))
        .rounded(px(4.0))
        .font_family(ctx.mono.clone())
        .text_size(px(ctx.font_size))
        .line_height(relative(ctx.mono_line_height))
        .when_some(info, |d, (line, colour)| d.child(div().text_color(colour).child(line)))
        .child(styled)
        .into_any_element()
}

/// A diagram block: once rendered, its PNG at its size in CSS pixels (the PNG has twice the
/// pixels) and at most the view's width; else its source in the code block style, uncoloured,
/// under an info line: "rendering" while the render waits or runs, the error in the tree's
/// deleted colour when it failed, and a word on installing Google Chrome when there is none.
fn diagram_element(d: &DiagramPiece, v: &Viewer, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let state = d.key.as_ref().and_then(|(_, key)| v.diagrams.get(key));
    let info: SharedString = match state {
        Some(Diagram::Ready { png, w, .. }) => {
            let (dim, border) = (ctx.dim, ctx.border);
            return img(png.clone()).w(px(*w as f32)).max_w_full().with_fallback(move || alt_box("diagram".into(), dim, border).into_any_element()).into_any_element();
        }
        Some(Diagram::Error(e)) => format!("diagram: {e}").into(),
        Some(Diagram::NoChrome) => "diagram: install Google Chrome to render".into(),
        Some(Diagram::Rendering) | None => "diagram: rendering".into(),
    };
    let colour = if matches!(state, Some(Diagram::Error(_))) { ctx.deleted } else { ctx.dim };
    code_element(&d.text, &d.drawn, None, Some((info, colour)), ctx, parts)
}

/// The runs of a code block's joined lines: a span's colour where the spans run contiguously
/// from the line's start, the text colour for the rest and for each newline, so the lengths always
/// sum to the text's.
fn code_runs(lines: &[String], spans: Option<&[Vec<Span>]>, ctx: &BlockCtx) -> Vec<TextRun> {
    let mono = font(ctx.mono.clone());
    let run = |len: usize, color: Hsla, bold: bool, italic: bool| {
        let mut f = mono.clone();
        f.weight = if bold { FontWeight::BOLD } else { FontWeight::NORMAL };
        f.style = if italic { FontStyle::Italic } else { FontStyle::Normal };
        TextRun { len, font: f, color, background_color: None, underline: None, strikethrough: None }
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut at = 0;
        for s in spans.and_then(|s| s.get(i)).map_or(&[][..], Vec::as_slice) {
            if s.start != at || s.end > line.len() || s.end <= s.start {
                break;
            }
            out.push(run(s.end - s.start, s.color.hsla(), s.bold, s.italic));
            at = s.end;
        }
        if at < line.len() {
            out.push(run(line.len() - at, ctx.text, false, false));
        }
        if i + 1 < lines.len() {
            out.push(run(1, ctx.text, false, false));
        }
    }
    out
}

/// A table as a grid: the header row semibold on the inactive tab colour, the body rows alternating
/// the window's background and a 4 percent tint of the text, 1 px hairlines between rows and
/// columns, each cell padded and aligned by its column, the columns as wide as `column_widths`
/// makes them for the view (a grid in `WideTables::Scroll` scrolls sideways in its block, the
/// vertical wheel still scrolling the list). A press on a cell puts the caret at the matching
/// source position (`Viewer::click_table_cell`), the block's own press standing aside through
/// `press`, as for a task box.
fn grid_element(ix: usize, grid: &Grid, widths: &Widths, ctx: &BlockCtx, press: &Rc<Cell<bool>>) -> AnyElement {
    ctx.drew_grid.set(true);
    let columns = grid.align.len();
    // The grid's own border takes 2 px of the view.
    let view = if ctx.view_w > 0.0 { ctx.view_w - 2.0 } else { f32::INFINITY };
    let natural = if widths.natural.len() == columns { widths.natural.clone() } else { vec![CELL_PAD_X * 2.0 + 1.0; columns] };
    let drawn = column_widths(&natural, view, ctx.wide_tables);
    let tint = ctx.text.opacity(0.04);
    let rows = grid.rows.iter().enumerate().map(|(r, row)| {
        let bg = match r {
            0 => ctx.code_bg,
            r if r % 2 == 1 => ctx.on_accent,
            _ => tint,
        };
        // The header is the block's first line, the body rows start below the delimiter row.
        let line = if r == 0 { 0 } else { r + 1 };
        let cells = row.iter().zip(&grid.align).zip(&drawn).enumerate().map(|(c, ((cell, align), w))| {
            let weight = if r == 0 { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
            let (text, layout) = runs_text(r * columns + c, &cell.runs, weight, ctx.text, ctx);
            let (viewer, press, cell_text, cell_runs) = (ctx.viewer.clone(), press.clone(), cell.text.clone(), cell.runs.clone());
            div()
                .flex_none()
                .w(px(*w))
                .min_w_0()
                .when(c > 0, |d| d.border_l_1().border_color(ctx.border))
                .px(px(CELL_PAD_X))
                .py(px(CELL_PAD_Y))
                .map(|d| match align {
                    Align::Right => d.text_right(),
                    Align::Center => d.text_center(),
                    Align::Default | Align::Left => d,
                })
                .child(text)
                .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _window, cx| {
                    // Cmd+click is for a link: the row leaves the caret alone for it too.
                    if ev.modifiers.platform {
                        return;
                    }
                    press.set(true);
                    let ch = if cell_text.is_empty() {
                        0
                    } else {
                        let byte = md_blocks::source_offset_at(&cell_runs, index_near(&layout, ev.position)).min(cell_text.len());
                        cell_text[..cell_text.floor_char_boundary(byte)].chars().count()
                    };
                    viewer.update(cx, |v, cx| v.click_table_cell(ix, line, c, ch, ev.modifiers.shift, cx));
                })
        });
        div().flex().flex_row().bg(bg).when(r > 0, |d| d.border_t_1().border_color(ctx.border)).children(cells)
    });
    let table = div()
        .flex_none()
        .flex()
        .flex_col()
        .w(px(drawn.iter().sum::<f32>() + 2.0))
        .border_1()
        .border_color(ctx.border)
        .font_family(ctx.ui.clone())
        .text_size(px(ctx.font_size * PARA))
        .line_height(relative(PARA_LINE))
        .children(rows);
    match ctx.wide_tables {
        WideTables::Wrap => table.into_any_element(),
        WideTables::Scroll => {
            let mut scroller = div().id(("md-table", ix)).w_full().overflow_x_scroll().track_scroll(&widths.scroll).child(table);
            // A vertical wheel over the table scrolls the list, not the table.
            scroller.style().restrict_scroll_to_axis = Some(true);
            scroller.into_any_element()
        }
    }
}

/// A table, HTML or front matter block: its source in the terminal font, on a background when
/// one is given.
fn mono_box(text: &SharedString, color: Hsla, bg: Option<Hsla>, ctx: &BlockCtx) -> AnyElement {
    div()
        .font_family(ctx.mono.clone())
        .text_size(px(ctx.font_size))
        .line_height(relative(ctx.mono_line_height))
        .text_color(color)
        .when_some(bg, |d, bg| d.bg(bg).p(px(CODE_PAD)).rounded(px(4.0)))
        .child(text.clone())
        .into_any_element()
}

/// An image at most the view's width (its natural size when smaller): from disk, or for a web
/// address from the cache once its fetch has landed (`Viewer.remote`). Its label (the alt text, or
/// the path) is shown in a dashed box instead while a fetch runs (with "loading"), after one
/// failed (with the url), and when the file cannot be drawn. A plain press is the row's, which puts
/// the caret in the block. Cmd+click follows the link around the image when there is one, as a
/// link's text does, and otherwise shows the file in Preview; the row stands aside through
/// `press`. The pointer is the hand over anything Cmd+click opens, as over a link.
fn image_element(image: &ImagePiece, v: &Viewer, ctx: &BlockCtx, press: &Rc<Cell<bool>>) -> AnyElement {
    let (label, path) = (&image.label, image.path.as_str());
    let (dim, border) = (ctx.dim, ctx.border);
    let alt_box = move |text: SharedString| alt_box(text, dim, border);
    let (file, shown) = if remote_image::is_remote(path) {
        match v.remote.get(path) {
            Some(Remote::Ready(file)) => (Some(file.clone()), label.clone()),
            Some(Remote::Failed) if label.as_ref() == path => (None, label.clone()),
            Some(Remote::Failed) => (None, format!("{label}: {path}").into()),
            Some(Remote::Fetching) | None => (None, format!("{label} (loading)").into()),
        }
    } else {
        (resolve_image(&ctx.base_dir, path), label.clone())
    };
    let on_press = {
        let (press, link, file, base_dir, viewer) = (press.clone(), image.link.clone(), file.clone(), ctx.base_dir.clone(), ctx.viewer.clone());
        move |ev: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
            if !ev.modifiers.platform {
                return;
            }
            match (&link, &file) {
                (Some(link), _) => follow_link(link_action(link, &base_dir), &viewer, cx),
                (None, Some(file)) => open_in_preview(file.clone(), cx),
                (None, None) => return,
            }
            press.set(true);
        }
    };
    match file {
        Some(file) => {
            let fallback = label.clone();
            img(file).max_w_full().with_fallback(move || alt_box(fallback.clone()).into_any_element()).cursor_pointer().on_mouse_down(MouseButton::Left, on_press).into_any_element()
        }
        // A linked image's box still follows the link while the image cannot be drawn.
        None if image.link.is_some() => alt_box(shown).cursor_pointer().on_mouse_down(MouseButton::Left, on_press).into_any_element(),
        None => alt_box(shown).into_any_element(),
    }
}

/// The dashed box an image, poster or media file's label is shown in when it cannot be drawn.
fn alt_box(text: SharedString, dim: Hsla, border: Hsla) -> Div {
    div().border_1().border_dashed().border_color(border).px(px(8.0)).py(px(4.0)).text_color(dim).child(text)
}

/// Shows an image in Preview through `/usr/bin/open` (`remote_image::preview_args`, the app
/// named), on the background executor. A path that is not absolute is not opened.
fn open_in_preview(file: PathBuf, cx: &mut App) {
    if !file.is_absolute() {
        return;
    }
    cx.background_executor().spawn(async move { run_open(&file, remote_image::preview_args(&file)) }).detach();
}

/// Opens a local video or audio file in QuickTime Player (`media::quicktime_args`) or shows a
/// local `webm` in Finder (`media::reveal_args`), on the background executor, never through a
/// bare `open`. First the path is resolved and checked (`media::checked_open_path`): it must be a
/// plain file of the kind its name says, so a link named like audio that leads to an app or a
/// script is refused, with a line on stderr and nothing shown in the window. A path that is not
/// absolute is not opened.
fn open_media(file: PathBuf, args: fn(&Path) -> Vec<String>, cx: &mut App) {
    if !file.is_absolute() {
        return;
    }
    cx.background_executor()
        .spawn(async move {
            match media::checked_open_path(&file) {
                Some(canonical) => run_open(&canonical, args(&canonical)),
                None => eprintln!("arkos: not opening {}: it is not a plain file of the kind its name says", file.display()),
            }
        })
        .detach();
}

/// Runs `/usr/bin/open` with `args` (made for `file`), blocking the calling thread, and reports a
/// failure on stderr.
fn run_open(file: &Path, args: Vec<String>) {
    let status = std::process::Command::new("/usr/bin/open").args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("arkos: cannot open {} ({s})", file.display()),
        Err(e) => eprintln!("arkos: cannot run /usr/bin/open: {e}"),
    }
}

/// A video, an audio file or a media link. A video is its poster at most the view's width with a
/// play glyph over its centre (`Viewer.posters`), or its label in the dashed box while the poster
/// is made (with "loading") and when there is none; audio is a one-line box with a speaker, the
/// file's name and its size (`Viewer.sizes`); a link (remote media, a `webm`) is its label and its
/// path in the link colour. A plain press is the row's, which puts the caret in the block.
/// Cmd+click follows the link around it when there is one, and otherwise plays a video or an audio
/// file in QuickTime Player, shows a local `webm` in Finder (both through `open_media`'s check),
/// or opens a web address in the browser; the row stands aside through `press`. The pointer is the hand over it, as over a link.
fn media_element(m: &MediaPiece, v: &Viewer, ctx: &BlockCtx, press: &Rc<Cell<bool>>) -> AnyElement {
    let remote = remote_image::is_remote(&m.path);
    let file = resolve_image(&ctx.base_dir, &m.path);
    let on_press = {
        let (press, link, kind, path, file, base_dir, viewer) = (press.clone(), m.link.clone(), m.kind, m.path.clone(), file.clone(), ctx.base_dir.clone(), ctx.viewer.clone());
        move |ev: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
            if !ev.modifiers.platform {
                return;
            }
            match (&link, kind, &file) {
                (Some(link), _, _) => follow_link(link_action(link, &base_dir), &viewer, cx),
                (None, _, None) if remote => cx.open_url(&path),
                (None, Some(_), Some(file)) => open_media(file.clone(), media::quicktime_args, cx),
                (None, None, Some(file)) => open_media(file.clone(), media::reveal_args, cx),
                (None, _, None) => return,
            }
            press.set(true);
        }
    };
    let shown: AnyElement = match (m.kind, &file) {
        (Some(Media::Video), Some(file)) => match v.posters.get(file) {
            Some(Poster::Ready(poster)) => poster_element(poster.clone(), m.label.clone(), ctx).into_any_element(),
            Some(Poster::Failed) => alt_box(m.label.clone(), ctx.dim, ctx.border).into_any_element(),
            Some(Poster::Making) | None => alt_box(format!("{} (loading)", m.label).into(), ctx.dim, ctx.border).into_any_element(),
        },
        (Some(Media::Audio), Some(file)) => {
            let name = file.file_name().map_or_else(|| m.path.clone(), |n| n.to_string_lossy().into_owned());
            let size = v.sizes.get(file).copied().flatten().map(media::size_text);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .border_1()
                .border_color(ctx.border)
                .rounded(px(4.0))
                .px(px(8.0))
                .py(px(4.0))
                .child(div().font_family(ctx.ui.clone()).child("\u{1F50A}"))
                .child(name)
                .when_some(size, |d, size| d.child(div().text_color(ctx.dim).child(size)))
                .into_any_element()
        }
        _ => {
            let text: SharedString = if m.label.as_ref() == m.path { m.label.clone() } else { format!("{} ({})", m.label, m.path).into() };
            div().text_color(ctx.link).underline().child(text).into_any_element()
        }
    };
    // The flex row keeps the box, the poster and the link as wide as they are, so the hand and
    // Cmd+click cover only them, not the rest of the line.
    div().flex().child(div().max_w_full().cursor_pointer().on_mouse_down(MouseButton::Left, on_press).child(shown)).into_any_element()
}

/// A video's poster at most the view's width (its natural size when smaller), with the play glyph
/// over its centre: a `PLAY_DISC` disc in the text colour at 60 percent around a triangle in the
/// window's background colour. A poster that cannot be drawn shows the label in the dashed box,
/// with no glyph over it, and so does one still loading (the glyph needs room for the disc).
fn poster_element(poster: PathBuf, label: SharedString, ctx: &BlockCtx) -> Div {
    let (dim, border, disc, glyph) = (ctx.dim, ctx.border, ctx.text.opacity(0.6), ctx.on_accent);
    // gpui asks for the fallback while it lays the image out, before anything is painted, so the
    // glyph's paint knows whether the poster was drawn.
    let failed = Rc::new(Cell::new(false));
    let fallback = {
        let failed = failed.clone();
        move || {
            failed.set(true);
            alt_box(label.clone(), dim, border).into_any_element()
        }
    };
    let play = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if failed.get() || bounds.size.width < px(PLAY_DISC) || bounds.size.height < px(PLAY_DISC) {
                return;
            }
            let centre = bounds.center();
            let half = px(PLAY_DISC / 2.0);
            let ring = Bounds { origin: point(centre.x - half, centre.y - half), size: size(px(PLAY_DISC), px(PLAY_DISC)) };
            window.paint_quad(gpui::fill(ring, disc).corner_radii(half));
            // The triangle sits in a `PLAY_TRIANGLE` box at the disc's centre, its point to the right.
            let corner = point(centre.x - px(PLAY_TRIANGLE / 2.0), centre.y - px(PLAY_TRIANGLE / 2.0));
            let at = |x: f32, y: f32| point(corner.x + px(x), corner.y + px(y));
            let mut b = PathBuilder::fill();
            b.move_to(at(4.0, 2.0));
            b.line_to(at(4.0, PLAY_TRIANGLE - 2.0));
            b.line_to(at(PLAY_TRIANGLE - 2.0, PLAY_TRIANGLE / 2.0));
            b.close();
            if let Ok(path) = b.build() {
                window.paint_path(path, glyph);
            }
        },
    );
    div()
        .relative()
        .flex()
        .max_w_full()
        .child(img(poster).max_w_full().with_fallback(fallback))
        .child(play.absolute().inset_0())
}

/// Where a raw block was laid out in a frame, kept in `Viewer.raw_blocks` for the input handler
/// and the mouse: the layout of its text (`raw.bounds` is the text's box, its width the wrap
/// width), the document lines it shows, and the first wrapped row of each of them with the total
/// row count last.
#[derive(Debug, Clone)]
pub struct BlockLayout {
    pub raw: RawLayout,
    pub lines: Range<usize>,
    pub row_tops: Vec<usize>,
}

impl BlockLayout {
    /// The window y below the block's last row.
    pub fn bottom(&self) -> Pixels {
        self.raw.bounds.origin.y + px(self.raw.line_h * self.row_tops.last().copied().unwrap_or(0) as f32)
    }

    /// The document position nearest a window point: the row under it (the first or last row for
    /// a point above or below the text), the column nearest it on that row.
    pub fn pos_at(&self, lines: &[String], at: Point<Pixels>, window: &mut Window) -> Pos {
        let lh = px(self.raw.line_h);
        let rows = self.row_tops.last().copied().unwrap_or(1).max(1);
        let rel_y = at.y - self.raw.bounds.origin.y;
        let row = ((rel_y / lh).floor().max(0.0) as usize).min(rows - 1);
        let i = self.row_tops.partition_point(|&t| t <= row).saturating_sub(1).min(self.lines.len().saturating_sub(1));
        let n = (self.lines.start + i).min(lines.len().saturating_sub(1));
        let Some(line) = lines.get(n) else { return Pos::default() };
        let (expanded, wrapped) = element::shape_raw_wrapped(window, line, &self.raw, self.raw.bounds.size.width);
        let within = point(at.x - self.raw.text_left, rel_y - lh * self.row_tops[i] as f32);
        Pos { line: n, ch: element::ch_for_col(line, element::wrapped_col_for_point(&wrapped, &expanded, within, lh)) }
    }

    /// The window point where position `p` is drawn (the top of its row), when its line is one
    /// this block shows.
    pub fn point_of(&self, lines: &[String], p: Pos, window: &mut Window) -> Option<Point<Pixels>> {
        let i = p.line.checked_sub(self.lines.start).filter(|_| self.lines.contains(&p.line))?;
        let line = lines.get(p.line)?;
        let lh = px(self.raw.line_h);
        let (expanded, wrapped) = element::shape_raw_wrapped(window, line, &self.raw, self.raw.bounds.size.width);
        let at = element::wrapped_point_for_col(&wrapped, &expanded, element::display_col(line, p.ch), lh);
        Some(point(self.raw.text_left + at.x, self.raw.bounds.origin.y + lh * self.row_tops[i] as f32 + at.y))
    }
}

/// How many wrapped rows each of `lines[shown]` takes at `wrap_width`. An ASCII line that fits
/// with a cell to spare is one row without shaping; any other line is shaped (gpui caches the
/// shape for the paint). With no usable width every line is one row.
fn row_counts(window: &mut Window, lines: &[String], shown: Range<usize>, layout: &RawLayout, wrap_width: Option<Pixels>) -> Vec<usize> {
    let run = TextRun { len: 1, font: font(layout.family.clone()), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
    let cell_w = f32::from(window.text_system().layout_line("W", px(layout.font_size), &[run], None).width);
    shown
        .map(|n| {
            let (Some(line), Some(width)) = (lines.get(n), wrap_width) else { return 1 };
            let expanded = element::expand_tabs(line);
            let (cut, _) = element::paint_cut(&expanded);
            if cut.is_ascii() && (cut.len() as f32 + 1.0) * cell_w <= f32::from(width) {
                return 1;
            }
            element::shape_raw_wrapped(window, line, layout, width).1.wrap_boundaries().len() + 1
        })
        .collect()
}

/// A block of the rendered view drawn as its source: the lines `shown` in the terminal family at
/// the terminal's size, coloured from `md_blocks::raw_marks`, wrapped to the row's width (its
/// height is the wrapped rows times the line height), with the caret, the selection, the marked
/// text and find hits painted as the raw view paints them. Only the lines inside the list's
/// visible area are painted. A press places the caret (Shift extends the selection) and a drag
/// selects; the text input itself is the list's canvas's.
pub struct RawBlockElement {
    viewer: Entity<Viewer>,
    lines: Arc<Vec<String>>,
    shown: Range<usize>,
    family: SharedString,
    font_size: f32,
    line_h: f32,
    colors: RawColors,
    caret: edit::Caret,
    marked: Option<Range<Pos>>,
    focused: bool,
    hits: Vec<Hit>,
    current: Option<Hit>,
    selection: Rgb,
    find_current: Rgb,
    caret_color: Rgb,
}

impl RawBlockElement {
    /// The text layout before it has bounds: the font, the size, the colours.
    fn raw_layout(&self, bounds: Bounds<Pixels>) -> RawLayout {
        RawLayout { bounds, text_left: bounds.origin.x, line_h: self.line_h, font_size: self.font_size, family: self.family.clone(), colors: self.colors }
    }
}

/// What a raw block's paint needs from its prepaint: where its rows start and the area that takes
/// the mouse (the text and the row's padding around it).
pub struct RawPrepaint {
    block: BlockLayout,
    hitbox: Hitbox,
    cell_w: f32,
}

impl Element for RawBlockElement {
    type RequestLayoutState = ();
    type PrepaintState = RawPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, _cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let (lines, shown, layout, line_h) = (self.lines.clone(), self.shown.clone(), self.raw_layout(Bounds::default()), self.line_h);
        let id = window.request_measured_layout(style, move |known, available, window, _cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let rows: usize = row_counts(window, &lines, shown.clone(), &layout, width.filter(|w| *w > px(0.0))).iter().sum();
            size(width.unwrap_or_default(), px(line_h * rows.max(1) as f32))
        });
        (id, ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) -> RawPrepaint {
        let layout = self.raw_layout(bounds);
        let counts = row_counts(window, &self.lines, self.shown.clone(), &layout, Some(bounds.size.width).filter(|w| *w > px(0.0)));
        let mut row_tops = Vec::with_capacity(counts.len() + 1);
        let mut at = 0;
        for c in &counts {
            row_tops.push(at);
            at += c;
        }
        row_tops.push(at);
        let text = Bounds { origin: bounds.origin, size: size(bounds.size.width, px(self.line_h * at.max(1) as f32)) };
        let block = BlockLayout { raw: self.raw_layout(text), lines: self.shown.clone(), row_tops };
        self.viewer.update(cx, |v, _| v.raw_blocks.push(block.clone()));
        // The row's padding takes the mouse too: a press beside the text is on its nearest column.
        let hit = Bounds { origin: point(bounds.origin.x - px(SIDE), bounds.origin.y), size: size(bounds.size.width + px(SIDE * 2.0), bounds.size.height + px(BLOCK_GAP)) };
        let hitbox = window.insert_hitbox(hit, HitboxBehavior::Normal);
        let run = TextRun { len: 1, font: font(self.family.clone()), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
        let cell_w = f32::from(window.text_system().layout_line("W", px(self.font_size), &[run], None).width);
        RawPrepaint { block, hitbox, cell_w }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut RawPrepaint, window: &mut Window, cx: &mut App) {
        let block = &prepaint.block;
        let lh = px(self.line_h);
        let x = bounds.origin.x;
        let mask = window.content_mask().bounds;
        let c = self.caret;
        let (lo, hi) = if c.anchor <= c.head { (c.anchor, c.head) } else { (c.head, c.anchor) };
        for (i, n) in self.shown.clone().enumerate() {
            let y = bounds.origin.y + lh * block.row_tops[i] as f32;
            let h = lh * (block.row_tops[i + 1] - block.row_tops[i]) as f32;
            // Only the lines inside the list's visible area are shaped and painted.
            if y + h < mask.top() || y > mask.bottom() {
                continue;
            }
            let Some(line) = self.lines.get(n) else { continue };
            let (expanded, wrapped) = element::shape_raw_wrapped(window, line, &block.raw, bounds.size.width);
            let end_col = expanded.chars().count();
            if lo != hi && lo.line <= n && n <= hi.line {
                let c0 = if n == lo.line { element::display_col(line, lo.ch) } else { 0 };
                let c1 = if n == hi.line { element::display_col(line, hi.ch) } else { end_col };
                for (x0, x1, dy) in element::wrapped_spans(&wrapped, &expanded, c0, c1, lh) {
                    element::paint_selection(window, x + x0, x + x1, y + dy, lh, self.selection);
                }
                // A selection running on past the line's end shows its line break as half a cell.
                if n < hi.line {
                    let end = element::wrapped_point_for_col(&wrapped, &expanded, end_col, lh);
                    element::paint_selection(window, x + end.x, x + end.x + px(prepaint.cell_w / 2.0), y + end.y, lh, self.selection);
                }
            }
            for hit in self.hits.iter().filter(|h| h.line == n) {
                let col = |byte: usize| element::display_col(line, line[..line.floor_char_boundary(byte.min(line.len()))].chars().count());
                let current = self.current == Some(*hit);
                for (x0, x1, dy) in element::wrapped_spans(&wrapped, &expanded, col(hit.start), col(hit.end), lh) {
                    element::paint_hits(window, &[(x + x0, x + x1, current)], y + dy, lh, self.selection, self.find_current);
                }
            }
            let _ = wrapped.paint(point(x, y), lh, TextAlign::Left, None, window, cx);
            if let Some(m) = self.marked.as_ref().filter(|m| m.start.line <= n && n <= m.end.line) {
                let c0 = if n == m.start.line { element::display_col(line, m.start.ch) } else { 0 };
                let c1 = if n == m.end.line { element::display_col(line, m.end.ch) } else { end_col };
                for (x0, x1, dy) in element::wrapped_spans(&wrapped, &expanded, c0, c1, lh) {
                    element::paint_marked(window, x + x0, x + x1, y + dy, lh, self.caret_color);
                }
            }
            if self.focused && c.head.line == n {
                let at = element::wrapped_point_for_col(&wrapped, &expanded, element::display_col(line, c.head.ch), lh);
                element::paint_caret(window, x + at.x, y + at.y, lh, self.caret_color);
            }
        }
        self.listen(prepaint, window);
    }
}

impl RawBlockElement {
    /// The mouse in the block: an I-beam, a press placing the caret (Shift extends the
    /// selection), a drag over the block moving the selection's head. A drag that leaves the block
    /// is followed by the rows it reaches; the canvas over the list ends it.
    fn listen(&self, prepaint: &RawPrepaint, window: &mut Window) {
        let hitbox = prepaint.hitbox.clone();
        window.set_cursor_style(CursorStyle::IBeam, &hitbox);
        let (v, block, lines, hb) = (self.viewer.clone(), prepaint.block.clone(), self.lines.clone(), hitbox.clone());
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left && hb.is_hovered(window) {
                let p = block.pos_at(&lines, ev.position, window);
                v.update(cx, |v, cx| v.place_caret(p, ev.modifiers.shift, cx));
            }
        });
        let (v, block, lines) = (self.viewer.clone(), prepaint.block.clone(), self.lines.clone());
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.pressed_button == Some(MouseButton::Left) && hitbox.is_hovered(window) && v.read(cx).is_selecting() {
                let p = block.pos_at(&lines, ev.position, window);
                v.update(cx, |v, cx| v.drag_to(p, cx));
            }
        });
    }
}

impl IntoElement for RawBlockElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// A vertical caret move in the rendered view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// The source of block `block`: its lines, the blank ones after it included, joined with a newline.
pub fn block_source(lines: &[String], block: &Block) -> String {
    let end = block.lines.end.min(lines.len());
    lines[block.lines.start.min(end)..end].join("\n")
}

/// The caret after Up or Down in the rendered view, where the blank lines after a block are not a
/// place for the caret: Down on the active block's last line with text (or below it) goes to the
/// start of the next block, Up on its first line to the end of the previous block's last line with
/// text, and any other move is a plain line move. In a table's rows a move without `select` goes
/// to the start of the same cell's text in the row above or below (over the delimiter row), and
/// the first or last row leaves the block as above. At the document's ends the caret stays. With
/// `select` the anchor stays, so the selection grows across blocks.
pub fn navigate(doc: &edit::Document, blocks: &[Block], caret: edit::Caret, dir: Dir, select: bool) -> edit::Caret {
    let m = match dir {
        Dir::Up => edit::Move::Up,
        Dir::Down => edit::Move::Down,
    };
    let lines = doc.lines();
    let head = doc.clamp(caret.head);
    let ix = md_blocks::block_at(blocks, head.line);
    let Some(block) = blocks.get(ix) else { return doc.move_by(caret, m, select) };
    // A table's first or last row: the move leaves the block.
    let mut table_edge = false;
    if !select && block.kind == BlockKind::Table {
        let source = block_source(lines, block);
        if let Some(pos) = md_table::cell_at(&source, head.line.saturating_sub(block.lines.start), head.ch) {
            let to = match dir {
                Dir::Up => md_table::cell_above(&source, pos),
                Dir::Down => md_table::cell_below(&source, pos),
            };
            match to.and_then(|to| md_table::cell_start(&source, to)) {
                Some((line, ch)) => return edit::Caret::at(Pos { line: block.lines.start + line, ch }),
                None => table_edge = true,
            }
        }
    }
    let across = match dir {
        Dir::Down if table_edge || head.line >= md_blocks::pos_of_offset(block, lines, usize::MAX).line => Some(blocks.get(ix + 1).map(|next| Pos { line: next.lines.start, ch: 0 })),
        Dir::Up if table_edge || head.line <= block.lines.start => Some(ix.checked_sub(1).map(|prev| md_blocks::pos_of_offset(&blocks[prev], lines, usize::MAX))),
        _ => None,
    };
    let anchor = |to: Pos| if select { doc.clamp(caret.anchor) } else { to };
    match across {
        None => doc.move_by(caret, m, select),
        Some(Some(to)) => edit::Caret { head: doc.clamp(to), anchor: anchor(doc.clamp(to)), goal_col: None },
        // The first block's first line, or the last block's last line with text: it stays.
        Some(None) => edit::Caret { head, anchor: anchor(head), goal_col: caret.goal_col },
    }
}

/// A press below the last block: the document gets blank lines at its end until it ends in two
/// (a paragraph needs one blank line after the text above it, and the caret the line after that),
/// as one undoable step, and the caret goes to the last line. A document that already ends in
/// two blank lines, or holds only blank lines, is left as it is.
pub fn append_paragraph(doc: &mut edit::Document, now: Instant) -> edit::Caret {
    doc.break_coalescing();
    let lines = doc.lines();
    let breaks = breaks_for_paragraph(lines);
    if breaks > 0 {
        let last = lines.len() - 1;
        let end = Pos { line: last, ch: lines[last].chars().count() };
        doc.replace_range(end, end, &"\n".repeat(breaks), now);
    }
    edit::Caret::at(Pos { line: doc.lines().len() - 1, ch: 0 })
}

/// The line breaks `append_paragraph` adds at the end of `lines`: as many as make them end in two
/// blank lines, none when they already do or hold only blank lines.
pub fn breaks_for_paragraph(lines: &[String]) -> usize {
    let blank_tail = lines.iter().rev().take_while(|l| l.trim().is_empty()).count();
    if blank_tail < 2 && blank_tail < lines.len() {
        2 - blank_tail
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md_blocks::split;

    #[test]
    fn column_widths_cap_shrink_to_fit_in_wrap_mode_and_keep_natural_in_scroll_mode() {
        let w = column_widths(&[100.0, 300.0, 800.0], 1000.0, WideTables::Wrap);
        assert_eq!(w, vec![100.0, 300.0, 450.0], "the cap is 45 percent of the view");
        let w = column_widths(&[600.0, 600.0, 600.0], 1000.0, WideTables::Wrap);
        assert!((w.iter().sum::<f32>() - 1000.0).abs() < 0.01, "shrunk to fit: {w:?}");
        assert!((w[0] - w[1]).abs() < 0.01);
        let w = column_widths(&[600.0, 600.0, 600.0], 1000.0, WideTables::Scroll);
        assert_eq!(w, vec![450.0, 450.0, 450.0], "scroll keeps the capped natural widths");
    }

    #[test]
    fn a_table_cell_draws_its_escaped_pipes_and_an_image_s_alt_and_a_long_table_stays_source() {
        let text = |cell: &str| GridCell::new(cell.to_string()).runs.iter().map(|r| r.text.as_str()).collect::<String>();
        assert_eq!(text("y \\| z"), "y | z");
        assert_eq!(text("`a\\|b`"), "a|b", "in a code span too");
        assert_eq!(text("![logo](l.png)"), "logo");
        assert_eq!(text("![a](a.png) ![b](https://x/b.svg)"), "a b", "a row of badges shows their alt texts");
        assert_eq!(text("- x"), "- x", "not a list");
        assert_eq!(text(""), "");
        assert!(matches!(TablePiece::new("| a |\n|---|\n| 1 |".into()), TablePiece::Grid(..)));
        assert!(matches!(TablePiece::new("| a |\n| 1 |".into()), TablePiece::Source(_)), "no delimiter row");
        let long = format!("| a |\n|---|{}", "\n| 1 |".repeat(GRID_MAX_ROWS + 1));
        assert!(matches!(TablePiece::new(long), TablePiece::Source(_)), "over the grid's rows");
    }

    #[test]
    fn video_audio_and_media_links_are_drawn_as_media_and_split_a_badge_row() {
        let pieces = |text: &str| {
            let mut out = Vec::new();
            for inline in md_blocks::inline(text, &BlockKind::Paragraph) {
                push_pieces(inline, &mut out);
            }
            out
        };
        let shape = |text: &str| {
            pieces(text)
                .iter()
                .map(|p| match p {
                    Piece::Image(i) => format!("image {}", i.path),
                    Piece::Images(row) => format!("row {}", row.len()),
                    Piece::Media(m) => format!("{:?} {}", m.kind, m.path),
                    _ => "other".to_string(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shape("![clip](v/clip.MOV)"), ["Some(Video) v/clip.MOV"]);
        assert_eq!(shape("![song](s.m4a)"), ["Some(Audio) s.m4a"]);
        assert_eq!(shape("![a](a.webm)"), ["None a.webm"], "a webm is a link");
        assert_eq!(shape("![a](a.png)"), ["image a.png"]);
        assert_eq!(shape("![a](a.svg) ![v](https://x/v.mp4) ![b](b.svg) ![c](c.svg)"), ["image a.svg", "None https://x/v.mp4", "row 2"]);
        let row = pieces("![a](https://x/a.svg) ![v](https://x/v.mp4)");
        let fetched: Vec<&String> = row.iter().flat_map(Piece::image_paths).collect();
        assert_eq!(fetched, ["https://x/a.svg"], "remote media is never fetched");
    }

    #[test]
    fn a_mermaid_block_is_a_diagram_and_other_code_stays_code() {
        let shape = |info: &str| {
            let mut out = Vec::new();
            for inline in md_blocks::inline("```x\ngraph TD; A-->B\n```", &BlockKind::CodeBlock { info: info.into() }) {
                push_pieces(inline, &mut out);
            }
            out.iter()
                .map(|p| match p {
                    Piece::Diagram(d) => format!("diagram {}", d.text.trim_end()),
                    Piece::Code { info, .. } => format!("code {info}"),
                    _ => "other".to_string(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shape("mermaid"), ["diagram graph TD; A-->B"]);
        assert_eq!(shape(" mermaid "), ["diagram graph TD; A-->B"]);
        assert_eq!(shape("mermaidjs"), ["code mermaidjs"]);
        assert_eq!(shape("rust"), ["code rust"]);
    }

    #[test]
    fn the_type_scale_follows_the_spec() {
        assert_eq!(scale(&BlockKind::Paragraph), PARA);
        assert_eq!(scale(&BlockKind::Heading(1)), PARA * HEADING[0]);
        assert_eq!(scale(&BlockKind::Heading(3)), PARA * HEADING[2]);
        assert_eq!(scale(&BlockKind::Heading(6)), PARA * HEADING[5]);
        assert_eq!(scale(&BlockKind::Heading(6)), 1.1, "the smallest heading is the paragraph size");
        assert_eq!(scale(&BlockKind::CodeBlock { info: String::new() }), 1.0, "code keeps the terminal size");
    }

    #[test]
    fn images_resolve_against_the_file_s_directory() {
        let base = Path::new("/p/docs");
        assert_eq!(resolve_image(base, "img/a.png"), Some(PathBuf::from("/p/docs/img/a.png")));
        assert_eq!(resolve_image(base, "/abs/b.png"), Some(PathBuf::from("/abs/b.png")));
        assert_eq!(resolve_image(base, "https://x.y/c.png"), None);
        assert_eq!(resolve_image(base, "../c.png"), Some(PathBuf::from("/p/c.png")), "normalised");
    }

    #[test]
    fn a_link_opens_the_web_shows_a_file_in_finder_or_opens_markdown_in_a_tab() {
        let base = Path::new("/p/docs");
        let url = |s: &str| LinkAction::OpenUrl(s.to_string());
        assert_eq!(link_action("https://example.com/a?b#c", base), url("https://example.com/a?b#c"));
        assert_eq!(link_action("HTTP://example.com", base), url("HTTP://example.com"));
        assert_eq!(link_action("mailto:me@example.com", base), url("mailto:me@example.com"));
        assert_eq!(link_action("other.md", base), LinkAction::OpenMarkdown(PathBuf::from("/p/docs/other.md")));
        assert_eq!(link_action("../Notes.MARKDOWN#part", base), LinkAction::OpenMarkdown(PathBuf::from("/p/Notes.MARKDOWN")), "the fragment goes");
        assert_eq!(link_action("my%20notes.md", base), LinkAction::OpenMarkdown(PathBuf::from("/p/docs/my notes.md")), "escapes decoded");
        assert_eq!(link_action("run.sh", base), LinkAction::Reveal(PathBuf::from("/p/docs/run.sh")), "never run");
        assert_eq!(link_action("/Applications/Calculator.app", base), LinkAction::Reveal(PathBuf::from("/Applications/Calculator.app")));
        assert_eq!(link_action("file:///tmp/x.command", base), LinkAction::Reveal(PathBuf::from("/tmp/x.command")), "a file URL is a path");
        assert_eq!(link_action("file://localhost/tmp/a%20b.md", base), LinkAction::OpenMarkdown(PathBuf::from("/tmp/a b.md")));
        assert_eq!(link_action("FILE:/tmp/y", base), LinkAction::Reveal(PathBuf::from("/tmp/y")));
        for nothing in ["#heading", "", "javascript:alert(1)", "x-apple-helpviewer://a", "vscode://file/a", "file://host/share"] {
            assert_eq!(link_action(nothing, base), LinkAction::Nothing, "{nothing:?}");
        }
        assert_eq!(percent_decode("a%2"), "a%2", "a cut escape stays");
        assert_eq!(percent_decode("%ff"), "%ff", "not UTF-8: kept as written");
    }

    #[test]
    fn a_code_block_is_coloured_only_within_the_limits() {
        assert!(should_highlight(100, 10, 1000));
        assert!(should_highlight(1000, HIGHLIGHT_MAX_LINES, 1000), "at both limits");
        assert!(!should_highlight(1001, 10, 1000), "over the configured bytes");
        assert!(!should_highlight(100, HIGHLIGHT_MAX_LINES + 1, usize::MAX), "over the line count");
    }

    #[test]
    fn block_navigation_crosses_blocks_and_clamps_at_the_ends() {
        let d = edit::Document::from_text("# H\n\npara one\npara two\n\n- item");
        let blocks = split(d.lines());
        let at = |l, c| edit::Caret { head: Pos { line: l, ch: c }, anchor: Pos { line: l, ch: c }, goal_col: None };
        assert_eq!(navigate(&d, &blocks, at(0, 1), Dir::Up, false).head, Pos { line: 0, ch: 1 }, "the first line of the first block stays");
        assert_eq!(navigate(&d, &blocks, at(5, 2), Dir::Down, false).head, Pos { line: 5, ch: 2 }, "the last line of the last block stays");
        assert_eq!(navigate(&d, &blocks, at(3, 4), Dir::Down, false).head, Pos { line: 5, ch: 0 }, "Down from the paragraph's last text line lands at the start of the list block");
        assert_eq!(navigate(&d, &blocks, at(5, 2), Dir::Up, false).head, Pos { line: 3, ch: 8 }, "Up from the list's first line lands at the end of the paragraph's last text line");
        assert_eq!(navigate(&d, &blocks, at(2, 4), Dir::Down, false).head, Pos { line: 3, ch: 4 }, "inside a block: a plain line move");
        let c = navigate(&d, &blocks, at(3, 4), Dir::Down, true);
        assert_eq!((c.head, c.anchor), (Pos { line: 5, ch: 0 }, Pos { line: 3, ch: 4 }), "Shift keeps the anchor across blocks");
    }

    #[test]
    fn up_and_down_move_by_cell_inside_a_table_and_leave_it_at_the_ends() {
        let d = edit::Document::from_text("intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nafter\n");
        let blocks = split(d.lines());
        let at = |l, c| edit::Caret::at(Pos { line: l, ch: c });
        assert_eq!(navigate(&d, &blocks, at(4, 6), Dir::Up, false).head, Pos { line: 2, ch: 6 }, "the same cell in the header, over the delimiter row");
        assert_eq!(navigate(&d, &blocks, at(2, 6), Dir::Down, false).head, Pos { line: 4, ch: 6 }, "the same cell in the body");
        assert_eq!(navigate(&d, &blocks, at(2, 3), Dir::Down, false).head, Pos { line: 4, ch: 2 }, "at the start of the cell's text");
        assert_eq!(navigate(&d, &blocks, at(2, 2), Dir::Up, false).head, Pos { line: 0, ch: 5 }, "the header falls through to the block move and leaves the table");
        assert_eq!(navigate(&d, &blocks, at(4, 2), Dir::Down, false).head, Pos { line: 6, ch: 0 }, "the last row leaves the table for the next block");
        let c = navigate(&d, &blocks, at(4, 6), Dir::Up, true);
        assert_eq!((c.head, c.anchor), (Pos { line: 3, ch: 6 }, Pos { line: 4, ch: 6 }), "a shifted arrow is a plain line move that selects");
        let d = edit::Document::from_text("| a | b |\n|---|---|\n\nafter\n");
        let blocks = split(d.lines());
        assert_eq!(navigate(&d, &blocks, at(0, 2), Dir::Down, false).head, Pos { line: 3, ch: 0 }, "a table with no body: Down from the header leaves it, never onto the delimiter row");
    }

    #[test]
    fn a_click_below_the_last_block_appends_a_paragraph() {
        let mut d = edit::Document::from_text("one");
        let c = append_paragraph(&mut d, Instant::now());
        assert_eq!(d.lines(), &["one".to_string(), String::new(), String::new()][..]);
        assert_eq!(c.head, Pos { line: 2, ch: 0 });
        let d2 = edit::Document::from_text("one\n\n");
        let mut d2 = d2;
        let c2 = append_paragraph(&mut d2, Instant::now());
        assert_eq!(d2.lines().len(), 3, "a document already ending in a blank line gets no second one");
        assert_eq!(c2.head.line, 2);
        let lines = |t: &str| edit::Document::from_text(t).lines().to_vec();
        assert_eq!(breaks_for_paragraph(&lines("one")), 2);
        assert_eq!(breaks_for_paragraph(&lines("one\n")), 1);
        assert_eq!(breaks_for_paragraph(&lines("one\n\n")), 0);
        assert_eq!(breaks_for_paragraph(&lines("")), 0, "only blank lines");
    }

    /// The first block of `text` with its code text as `md_blocks::inline` gives it.
    fn code_block(text: &str) -> (Vec<String>, Block, String) {
        let d = edit::Document::from_text(text);
        let lines = d.lines().to_vec();
        let block = split(&lines)[0].clone();
        let code = match &md_blocks::inline(&lines[block.lines.clone()].join("\n"), &block.kind)[0] {
            Inline::Code { text, .. } => text.clone(),
            other => panic!("not code: {other:?}"),
        };
        (lines, block, code)
    }

    #[test]
    fn a_press_in_a_code_block_maps_to_its_source_line_and_column() {
        let (lines, block, code) = code_block("```rust\nfn a() {}\n    x\n```");
        assert_eq!(code_pos(&block, &lines, &code, 3), Pos { line: 1, ch: 3 }, "a fenced block's first row is the line below the fence");
        assert_eq!(code_pos(&block, &lines, &code, 12), Pos { line: 2, ch: 2 }, "the second row, inside its indent");
        assert_eq!(code_pos(&block, &lines, &code, 99), Pos { line: 2, ch: 5 }, "past the end: the last code line's end");
        let (lines, block, code) = code_block("    let a = 1;\n    b");
        assert_eq!(code_pos(&block, &lines, &code, 4), Pos { line: 0, ch: 8 }, "an indented block's column is past the four-space indent");
        assert_eq!(code_pos(&block, &lines, &code, 11), Pos { line: 1, ch: 4 });
        let (lines, block, code) = code_block("```\n\tx\n```");
        assert_eq!(code_pos(&block, &lines, &code, 4), Pos { line: 1, ch: 1 }, "a tab is drawn four columns wide: after it is the x");
        assert_eq!(code_pos(&block, &lines, &code, 1), Pos { line: 1, ch: 0 }, "in the tab's left half: before it");
    }
}
