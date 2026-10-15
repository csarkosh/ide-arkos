//! The rendered markdown view (spec 7): one row of a virtualised gpui `list` per top-level block,
//! each block drawn from its inline runs (`md_blocks::inline`), cached per block in the viewer
//! and dropped only for the blocks an edit changed. Paragraphs are in the UI font, code and tables
//! in the terminal's; a task item's box toggles the item through an undoable edit, and a link
//! opens on Cmd+click.

use crate::edit;
use crate::highlight::Highlighter;
use crate::md_blocks::{self, BlockKind, Inline, Run};
use crate::theme::Theme;
use crate::viewer::{Span, Viewer};
use gpui::{
    div, font, img, list, prelude::*, px, relative, AnyElement, App, Context, Entity, FontStyle, FontWeight, Hsla, InteractiveText, MouseButton, MouseDownEvent, SharedString, StrikethroughStyle,
    StyledText, TextLayout, TextRun, UnderlineStyle,
};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

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

/// A code block's lines as drawn: the trailing newline dropped and tabs expanded.
fn code_lines(text: &str) -> Vec<String> {
    text.strip_suffix('\n').unwrap_or(text).split('\n').map(|l| l.replace('\t', "    ")).collect()
}

/// Fills block `ix`'s cache entry if it is empty, and colours a code block once the highlighter is
/// there (again when it changed). Nothing is computed for a block already cached.
pub fn fill(v: &mut Viewer, ix: usize, highlighter: Option<&Arc<Highlighter>>) {
    let (Some(doc), Some(block), Some(slot)) = (v.doc.as_ref(), v.blocks.get(ix), v.block_cache.get_mut(ix)) else { return };
    let r = slot.get_or_insert_with(|| {
        let lines = doc.lines();
        let end = block.lines.end.min(lines.len());
        let text = lines[block.lines.start.min(end)..end].join("\n");
        Rendered { inlines: md_blocks::inline(&text, &block.kind), code_spans: None }
    });
    if let (Some(h), Some(Inline::Code { info, text })) = (highlighter, r.inlines.first()) {
        if !r.code_spans.as_ref().is_some_and(|(by, _)| Arc::ptr_eq(by, h)) {
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
    pub text: Hsla,
    pub dim: Hsla,
    pub link: Hsla,
    pub code_bg: Hsla,
    pub border: Hsla,
    pub on_accent: Hsla,
}

/// The rendered view of a document: a `list` with one row per block, each built from the block's
/// cached rendering (filled on demand). The list scrolls itself and keeps its place in
/// `Viewer.list_state`.
pub fn render(viewer: &Entity<Viewer>, theme: &Theme, ui_family: SharedString, mono_family: SharedString, font_size: f32, highlighter: Option<Arc<Highlighter>>, cx: &App) -> AnyElement {
    let v = viewer.read(cx);
    let state = v.list_state.clone();
    let ctx = Rc::new(BlockCtx {
        viewer: viewer.clone(),
        ui: ui_family,
        mono: mono_family,
        font_size,
        mono_line_height: v.line_height,
        base_dir: v.path.parent().map(Path::to_path_buf).unwrap_or_default(),
        text: theme.chrome.text.hsla(),
        dim: theme.chrome.text_dim.hsla(),
        link: theme.chrome.focus_ring.hsla(),
        code_bg: theme.chrome.tab_inactive.hsla(),
        border: theme.chrome.border.hsla(),
        on_accent: theme.chrome.window_bg.hsla(),
    });
    div()
        .id(("md-view", viewer.entity_id()))
        .size_full()
        .child(
            list(state, move |ix, _window, cx| {
                // `render_item` gets the App, not the viewer: the cache is filled through the
                // entity, then read while the row is built.
                ctx.viewer.update(cx, |v, _| fill(v, ix, highlighter.as_ref()));
                let v = ctx.viewer.read(cx);
                match (v.blocks.get(ix), v.block_cache.get(ix).and_then(Option::as_ref)) {
                    (Some(block), Some(r)) => block_element(ix, &block.kind, r, &ctx),
                    _ => div().into_any_element(),
                }
            })
            .size_full(),
        )
        .into_any_element()
}

/// One block's row: its inlines in the block's type, 6 px of space below. Every text element's
/// layout is kept on the row's mouse-down for the caret placement of Task 6.
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
    if *kind == BlockKind::Blank {
        return row.child(div().h(px(BLANK_H))).into_any_element();
    }
    let mut layouts: Vec<TextLayout> = Vec::new();
    let mut children: Vec<AnyElement> = Vec::with_capacity(r.inlines.len());
    for (j, inline) in r.inlines.iter().enumerate() {
        let child = match inline {
            Inline::Para(runs) => text_element(j, runs, FontWeight::NORMAL, ctx.text, ctx, &mut layouts),
            Inline::Heading(level, runs) => div()
                .pt(px(HEADING_GAP))
                .text_size(px(ctx.font_size * heading_scale(*level)))
                .child(text_element(j, runs, FontWeight::SEMIBOLD, ctx.text, ctx, &mut layouts))
                .into_any_element(),
            Inline::Quote(runs) => div()
                .border_l(px(QUOTE_BAR))
                .border_color(ctx.border)
                .pl(px(10.0))
                .child(text_element(j, runs, FontWeight::NORMAL, ctx.dim, ctx, &mut layouts))
                .into_any_element(),
            Inline::Item { depth, marker, task, runs } => item_element(ix, j, *depth, marker, *task, runs, size, ctx, &mut layouts),
            Inline::Code { text, .. } => code_element(text, r.code_spans.as_ref().map(|(_, s)| s.as_slice()), ctx, &mut layouts),
            Inline::Rule => div().my(px(RULE_GAP)).h(px(1.0)).w_full().bg(ctx.border).into_any_element(),
            Inline::Table(t) | Inline::Html(t) => mono_box(t, ctx.text, Some(ctx.code_bg), ctx),
            Inline::Metadata(t) => mono_box(t, ctx.dim, None, ctx),
            Inline::Image { alt, path } => image_element(alt, path, ctx),
        };
        children.push(child);
    }
    row.children(children)
        // Task 6 maps a press here to a source position through `layouts` (and
        // `md_blocks::source_offset_at` over the block's runs); this task leaves it a no-op.
        .on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _window, _cx| {
            let _ = &layouts;
        })
        .into_any_element()
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
fn text_element(j: usize, runs: &[Run], weight: FontWeight, color: Hsla, ctx: &BlockCtx, layouts: &mut Vec<TextLayout>) -> AnyElement {
    let (text, text_runs, links) = styled_runs(runs, weight, color, ctx);
    let styled = StyledText::new(text).with_runs(text_runs);
    layouts.push(styled.layout().clone());
    if links.is_empty() {
        return styled.into_any_element();
    }
    let (ranges, targets): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    let base_dir = ctx.base_dir.clone();
    InteractiveText::new(("md-text", j), styled)
        .on_click(ranges, move |i, window, cx| {
            // A plain click is for the text (the caret, from Task 6); Cmd+click follows the link.
            if window.modifiers().platform {
                if let Some(target) = targets.get(i) {
                    open_link(target, &base_dir, cx);
                }
            }
        })
        .into_any_element()
}

/// Follows a link: a web address (or any URL with a scheme, and mail) in the browser, a path with
/// the system's application for it, resolved against the file's directory. A link to a heading
/// in the same file (`#...`) does nothing yet.
fn open_link(target: &str, base_dir: &Path, cx: &mut App) {
    if target.contains("://") || target.starts_with("mailto:") {
        cx.open_url(target);
        return;
    }
    let path = target.split('#').next().unwrap_or("");
    if path.is_empty() {
        return;
    }
    if let Some(resolved) = resolve_image(base_dir, path) {
        cx.open_with_system(&resolved);
    }
}

/// A list item: the marker column (a bullet, or the number with its delimiter) indented by its
/// depth, a task's box, and the item's text.
#[allow(clippy::too_many_arguments)]
fn item_element(ix: usize, j: usize, depth: u8, marker: &str, task: Option<bool>, runs: &[Run], size: f32, ctx: &BlockCtx, layouts: &mut Vec<TextLayout>) -> AnyElement {
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
            .when_some(offset, move |d, offset| d.on_click(move |_ev, _window, cx| viewer.update(cx, |v, cx| toggle_item(v, ix, offset, cx))))
    });
    div()
        .flex()
        .flex_row()
        .items_start()
        .pl(px(f32::from(depth) * 2.0 * ch))
        .child(div().flex_none().min_w(px(2.0 * ch)).pr(px(ch * 0.5)).text_color(ctx.dim).child(bullet))
        .children(check)
        .child(div().flex_1().min_w_0().child(text_element(j, runs, FontWeight::NORMAL, ctx.text, ctx, layouts)))
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
fn code_element(text: &str, spans: Option<&[Vec<Span>]>, ctx: &BlockCtx, layouts: &mut Vec<TextLayout>) -> AnyElement {
    let lines = code_lines(text);
    let runs = code_runs(&lines, spans, ctx);
    let styled = StyledText::new(lines.join("\n")).with_runs(runs);
    layouts.push(styled.layout().clone());
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
