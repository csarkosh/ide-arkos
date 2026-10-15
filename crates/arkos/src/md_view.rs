//! The rendered markdown view (spec 7): one row of a virtualised gpui `list` per top-level block,
//! each block drawn from its inline runs (`md_blocks::inline`), cached per block in the viewer
//! and dropped only for the blocks an edit changed. Paragraphs are in the UI font, code and tables
//! in the terminal's; a task item's box toggles the item through an undoable edit, and a link
//! opens on Cmd+click.
//!
//! The block holding the caret (and every block a selection touches) is drawn raw instead, by
//! `RawBlockElement`: its source lines in the terminal font, wrapped to the view's width, with the
//! caret, the selection and the marked text. A click on a rendered block puts the caret at the
//! source position under it, Up and Down cross from block to block (`navigate`), and the input
//! handler sits on a canvas over the whole list, so typing reaches the document even when the
//! caret's block is scrolled out of view.

use crate::edit::{self, Pos};
use crate::highlight::Highlighter;
use crate::md_blocks::{self, Block, BlockKind, Inline, Run};
use crate::palette::Rgb;
use crate::theme::Theme;
use crate::viewer::{self, Hit, Span, Viewer, ViewerEvent};
use crate::viewer_element::{self as element, RawColors, RawLayout};
use gpui::{
    canvas, div, font, img, list, point, prelude::*, px, relative, size, AnyElement, App, AvailableSpace, Bounds, Context, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler,
    Entity, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, InteractiveText, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, SharedString, StrikethroughStyle, Style, StyledText, TextAlign, TextLayout, TextRun, UnderlineStyle, Window,
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

/// The size of a block's text as a multiple of the font size: paragraphs, lists and quotes at
/// `PARA`, headings at `PARA` times their level's step, code, tables, HTML and front matter at the
/// terminal's size.
pub fn scale(kind: &BlockKind) -> f32 {
    match kind {
        BlockKind::Heading(level) => heading_scale(*level),
        BlockKind::CodeBlock { .. } | BlockKind::Table | BlockKind::Html | BlockKind::Metadata => 1.0,
        BlockKind::Paragraph | BlockKind::List | BlockKind::Quote | BlockKind::Rule | BlockKind::Blank => PARA,
    }
}

fn heading_scale(level: u8) -> f32 {
    PARA * HEADING[usize::from(level.clamp(1, 6) - 1)]
}

/// Where an image (or a linked file) is on disk: a relative path against the markdown file's
/// directory, with `.` and `..` resolved lexically; an absolute path as it is. A web address has
/// no path.
pub fn resolve_image(base_dir: &Path, path: &str) -> Option<PathBuf> {
    let lower = path.get(..8).unwrap_or(path).to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
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

/// One block's rendering, cached in `Viewer.block_cache`: its inline content, and for a code block
/// the colours of its lines with the highlighter that made them (a theme change makes a new
/// highlighter, and the colours are made again).
pub struct Rendered {
    pub inlines: Vec<Inline>,
    pub code_spans: Option<(Arc<Highlighter>, Vec<Vec<Span>>)>,
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

/// Fills block `ix`'s cache entry if it is empty, and colours a code block once the highlighter is
/// there (again when it changed), unless it is over the highlight limit `limit` or
/// `HIGHLIGHT_MAX_LINES` (`should_highlight`). Nothing is computed for a block already cached.
pub fn fill(v: &mut Viewer, ix: usize, highlighter: Option<&Arc<Highlighter>>, limit: usize) {
    let (Some(doc), Some(block), Some(slot)) = (v.doc.as_ref(), v.blocks.get(ix), v.block_cache.get_mut(ix)) else { return };
    let r = slot.get_or_insert_with(|| {
        let lines = doc.lines();
        let end = block.lines.end.min(lines.len());
        let text = lines[block.lines.start.min(end)..end].join("\n");
        Rendered { inlines: md_blocks::inline(&text, &block.kind), code_spans: None }
    });
    if let (Some(h), Some(Inline::Code { info, text })) = (highlighter, r.inlines.first()) {
        if !r.code_spans.as_ref().is_some_and(|(by, _)| Arc::ptr_eq(by, h)) && should_highlight(text.len(), text.lines().count(), limit) {
            r.code_spans = Some((h.clone(), h.highlight_token(info, &code_lines(text))));
        }
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
    pub text: Hsla,
    pub dim: Hsla,
    pub link: Hsla,
    pub code_bg: Hsla,
    pub border: Hsla,
    pub on_accent: Hsla,
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
/// A code block over `highlight_limit` bytes (`viewer.highlight_limit_bytes`) is drawn plain.
#[allow(clippy::too_many_arguments)]
pub fn render(viewer: &Entity<Viewer>, theme: &Theme, ui_family: SharedString, mono_family: SharedString, font_size: f32, highlighter: Option<Arc<Highlighter>>, highlight_limit: usize, cx: &mut App) -> AnyElement {
    // The raw blocks of this frame record their layouts as they are laid out.
    viewer.update(cx, |v, _| v.raw_blocks.clear());
    let v = viewer.read(cx);
    let state = v.list_state.clone();
    let ctx = Rc::new(BlockCtx {
        viewer: viewer.clone(),
        ui: ui_family,
        mono: mono_family,
        font_size,
        mono_line_height: v.line_height,
        base_dir: v.path.parent().map(Path::to_path_buf).unwrap_or_default(),
        highlight_limit,
        text: theme.chrome.text.hsla(),
        dim: theme.chrome.text_dim.hsla(),
        link: theme.chrome.focus_ring.hsla(),
        code_bg: theme.chrome.tab_inactive.hsla(),
        border: theme.chrome.border.hsla(),
        on_accent: theme.chrome.window_bg.hsla(),
        raw: RawColors { text: theme.chrome.text, dim: theme.chrome.text_dim, code: theme.tree.modified, link: theme.chrome.focus_ring },
        selection: theme.semantic.find_match,
        find_current: theme.semantic.find_current,
        caret: theme.chrome.text,
    });
    let input = input_canvas(viewer.clone());
    div()
        .id(("md-view", viewer.entity_id()))
        .relative()
        .size_full()
        .child(
            list(state, move |ix, window, cx| {
                // The caret's block is raw only while the viewer has the focus: gpui redraws the
                // window when the focus moves, so a blur turns it back to rendered.
                let (count, raw) = {
                    let v = ctx.viewer.read(cx);
                    (v.blocks.len(), v.renders_raw(ix, v.focus.is_focused(window)))
                };
                if ix >= count {
                    return tail_element(&ctx);
                }
                if raw {
                    return raw_row(ix, &ctx, window, cx);
                }
                // `render_item` gets the App, not the viewer: the cache is filled through the
                // entity, then read while the row is built.
                ctx.viewer.update(cx, |v, _| fill(v, ix, highlighter.as_ref(), ctx.highlight_limit));
                let v = ctx.viewer.read(cx);
                match (v.blocks.get(ix), v.block_cache.get(ix).and_then(Option::as_ref)) {
                    (Some(block), Some(r)) => block_element(ix, &block.kind, r, &ctx),
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
/// presses, so the rows below it get them.
fn input_canvas(viewer: Entity<Viewer>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, cx| {
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

/// The row below the last block, two paragraph lines high: a press there starts a new paragraph
/// at the end of the document and puts the caret in it.
fn tail_element(ctx: &BlockCtx) -> AnyElement {
    let viewer = ctx.viewer.clone();
    div()
        .id("md-tail")
        .w_full()
        .h(px(ctx.font_size * PARA * PARA_LINE * 2.0))
        .cursor_text()
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
    Runs(Vec<Run>),
    Code(String),
}

/// One block's row: its inlines in the block's type, 6 px of space below. A press puts the caret
/// at the source position under it (Shift extends the selection); a drag that reaches the row
/// moves the selection's head there.
pub fn block_element(ix: usize, kind: &BlockKind, r: &Rendered, ctx: &BlockCtx) -> AnyElement {
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
    let mut children: Vec<AnyElement> = Vec::with_capacity(r.inlines.len().max(1));
    if *kind == BlockKind::Blank {
        children.push(div().h(px(BLANK_H)).into_any_element());
    }
    for (j, inline) in r.inlines.iter().enumerate() {
        let child = match inline {
            Inline::Para(runs) => text_element(j, runs, FontWeight::NORMAL, ctx.text, ctx, &mut parts),
            Inline::Heading(level, runs) => div()
                .pt(px(HEADING_GAP))
                .text_size(px(ctx.font_size * heading_scale(*level)))
                .child(text_element(j, runs, FontWeight::SEMIBOLD, ctx.text, ctx, &mut parts))
                .into_any_element(),
            Inline::Quote(runs) => div()
                .border_l(px(QUOTE_BAR))
                .border_color(ctx.border)
                .pl(px(10.0))
                .child(text_element(j, runs, FontWeight::NORMAL, ctx.dim, ctx, &mut parts))
                .into_any_element(),
            Inline::Item { depth, marker, task, runs } => item_element(ix, j, *depth, marker, *task, runs, size, ctx, &mut parts),
            Inline::Code { text, .. } => code_element(text, r.code_spans.as_ref().map(|(_, s)| s.as_slice()), ctx, &mut parts),
            Inline::Rule => div().my(px(RULE_GAP)).h(px(1.0)).w_full().bg(ctx.border).into_any_element(),
            Inline::Table(t) | Inline::Html(t) => mono_box(t, ctx.text, Some(ctx.code_bg), ctx),
            Inline::Metadata(t) => mono_box(t, ctx.dim, None, ctx),
            Inline::Image { alt, path } => image_element(alt, path, ctx),
        };
        children.push(child);
    }
    let Parts { texts, task_press } = parts;
    let texts = Rc::new(texts);
    let (v_down, v_move, texts_down) = (ctx.viewer.clone(), ctx.viewer.clone(), texts.clone());
    row.children(children)
        .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _window, cx| {
            // A task box's press is for the box, and Cmd+click is for a link: the block stays
            // rendered so their clicks land on the button's release.
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
            if ev.pressed_button != Some(MouseButton::Left) {
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
    let b = layout.bounds();
    let inside = point(at.x.max(b.left()).min((b.right() - px(1.0)).max(b.left())), at.y.max(b.top()).min((b.bottom() - px(1.0)).max(b.top())));
    let byte = layout.index_for_position(inside).unwrap_or_else(|e| e);
    Some(match part {
        Part::Runs(runs) => md_blocks::pos_of_offset(block, lines, md_blocks::source_offset_at(runs, byte)),
        Part::Code(text) => code_pos(block, lines, text, byte),
    })
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

/// A run list as text: a `StyledText`, wrapped to open its links on Cmd+click when it has any.
fn text_element(j: usize, runs: &[Run], weight: FontWeight, color: Hsla, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let (text, text_runs, links) = styled_runs(runs, weight, color, ctx);
    let styled = StyledText::new(text).with_runs(text_runs);
    parts.texts.push((styled.layout().clone(), Part::Runs(runs.to_vec())));
    if links.is_empty() {
        return styled.into_any_element();
    }
    let (ranges, targets): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    let (base_dir, viewer) = (ctx.base_dir.clone(), ctx.viewer.clone());
    InteractiveText::new(("md-text", j), styled)
        .on_click(ranges, move |i, window, cx| {
            // A plain click is for the text (the row put the caret there); Cmd+click follows the
            // link.
            if window.modifiers().platform {
                if let Some(target) = targets.get(i) {
                    follow_link(link_action(target, &base_dir), &viewer, cx);
                }
            }
        })
        .into_any_element()
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
fn item_element(ix: usize, j: usize, depth: u8, marker: &str, task: Option<bool>, runs: &[Run], size: f32, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
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
/// highlighter's spans when they are there.
fn code_element(text: &str, spans: Option<&[Vec<Span>]>, ctx: &BlockCtx, parts: &mut Parts) -> AnyElement {
    let lines = code_lines(text);
    let runs = code_runs(&lines, spans, ctx);
    let styled = StyledText::new(lines.join("\n")).with_runs(runs);
    parts.texts.push((styled.layout().clone(), Part::Code(text.to_string())));
    div()
        .bg(ctx.code_bg)
        .p(px(CODE_PAD))
        .rounded(px(4.0))
        .font_family(ctx.mono.clone())
        .text_size(px(ctx.font_size))
        .line_height(relative(ctx.mono_line_height))
        .child(styled)
        .into_any_element()
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

/// A table, HTML or front matter block: its source in the terminal font, on a background when
/// one is given.
fn mono_box(text: &str, color: Hsla, bg: Option<Hsla>, ctx: &BlockCtx) -> AnyElement {
    div()
        .font_family(ctx.mono.clone())
        .text_size(px(ctx.font_size))
        .line_height(relative(ctx.mono_line_height))
        .text_color(color)
        .when_some(bg, |d, bg| d.bg(bg).p(px(CODE_PAD)).rounded(px(4.0)))
        .child(text.to_string())
        .into_any_element()
}

/// An image from disk at most the view's width; its alt text (or path) in a dashed box when it
/// cannot be loaded or is on the web.
fn image_element(alt: &str, path: &str, ctx: &BlockCtx) -> AnyElement {
    let label: SharedString = if alt.is_empty() { path.to_string().into() } else { alt.to_string().into() };
    let (dim, border) = (ctx.dim, ctx.border);
    let alt_box = move || div().border_1().border_dashed().border_color(border).px(px(8.0)).py(px(4.0)).text_color(dim).child(label.clone()).into_any_element();
    match resolve_image(&ctx.base_dir, path) {
        Some(resolved) => img(resolved).max_w_full().with_fallback(alt_box).into_any_element(),
        None => alt_box(),
    }
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

/// The caret after Up or Down in the rendered view, where the blank lines after a block are not a
/// place for the caret: Down on the active block's last line with text (or below it) goes to the
/// start of the next block, Up on its first line to the end of the previous block's last line with
/// text, and any other move is a plain line move. At the document's ends the caret stays. With
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
    let across = match dir {
        Dir::Down if head.line >= md_blocks::pos_of_offset(block, lines, usize::MAX).line => Some(blocks.get(ix + 1).map(|next| Pos { line: next.lines.start, ch: 0 })),
        Dir::Up if head.line <= block.lines.start => Some(ix.checked_sub(1).map(|prev| md_blocks::pos_of_offset(&blocks[prev], lines, usize::MAX))),
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
    let blank_tail = lines.iter().rev().take_while(|l| l.trim().is_empty()).count();
    if blank_tail < 2 && blank_tail < lines.len() {
        let last = lines.len() - 1;
        let end = Pos { line: last, ch: lines[last].chars().count() };
        doc.replace_range(end, end, &"\n".repeat(2 - blank_tail), now);
    }
    edit::Caret::at(Pos { line: doc.lines().len() - 1, ch: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md_blocks::split;

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
