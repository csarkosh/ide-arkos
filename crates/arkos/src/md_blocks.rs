//! The markdown layer of the editor: a document split into top-level blocks with source ranges,
//! the inline runs of one block with their source offsets, list and task helpers, and a light
//! per-line colourer for the raw text of the block under the caret. Pure text, no gpui.
use crate::edit;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// The dialect the editor reads: tables, task lists, strikethrough and front matter.
pub const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    List,
    CodeBlock { info: String },
    Quote,
    Rule,
    Table,
    Html,
    Metadata,
    Blank,
}

/// A run of whole lines. Blank lines after a block belong to it; blank lines before the first
/// block are a `Blank` block of their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub lines: Range<usize>,
    pub kind: BlockKind,
}

/// Splits the document into top-level blocks: block i covers its start line up to block i+1's
/// start, so every line is in exactly one block. An empty slice has no blocks; a document with
/// no content is one `Blank` block.
pub fn split(lines: &[String]) -> Vec<Block> {
    if lines.is_empty() {
        return Vec::new();
    }
    let text = lines.join("\n");
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for l in lines {
        line_starts.push(at);
        at += l.len() + 1;
    }
    let line_of = |offset: usize| line_starts.partition_point(|&s| s <= offset) - 1;
    let mut found: Vec<(usize, BlockKind)> = Vec::new();
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(&text, OPTIONS).into_offset_iter() {
        let kind = match event {
            Event::Start(tag) => {
                depth += 1;
                if depth != 1 {
                    continue;
                }
                match tag {
                    Tag::Heading { level, .. } => BlockKind::Heading(level as u8),
                    Tag::List(_) => BlockKind::List,
                    Tag::CodeBlock(CodeBlockKind::Fenced(info)) => BlockKind::CodeBlock { info: info.trim().to_string() },
                    Tag::CodeBlock(CodeBlockKind::Indented) => BlockKind::CodeBlock { info: String::new() },
                    Tag::BlockQuote(_) => BlockKind::Quote,
                    Tag::Table(_) => BlockKind::Table,
                    Tag::HtmlBlock => BlockKind::Html,
                    Tag::MetadataBlock(_) => BlockKind::Metadata,
                    _ => BlockKind::Paragraph,
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Event::Rule if depth == 0 => BlockKind::Rule,
            _ => continue,
        };
        let line = line_of(range.start);
        if found.last().is_some_and(|(l, _)| *l == line) {
            continue;
        }
        found.push((line, kind));
    }
    let mut blocks = Vec::with_capacity(found.len() + 1);
    match found.first() {
        None => return vec![Block { lines: 0..lines.len(), kind: BlockKind::Blank }],
        Some((first, _)) if *first > 0 => blocks.push(Block { lines: 0..*first, kind: BlockKind::Blank }),
        Some(_) => {}
    }
    for (i, (start, kind)) in found.iter().enumerate() {
        let end = found.get(i + 1).map_or(lines.len(), |(next, _)| *next);
        blocks.push(Block { lines: *start..end, kind: kind.clone() });
    }
    blocks
}

/// The index of the block holding `line`; past the end, the last block.
pub fn block_at(blocks: &[Block], line: usize) -> usize {
    blocks.partition_point(|b| b.lines.end <= line).min(blocks.len().saturating_sub(1))
}

/// The blocks an edit replaced: the common prefix and suffix of the old and new structures leave
/// `range` in the old list, replaced by `count` new blocks. Two blocks are equal when their kind,
/// their line count and their joined text match, so a block that only moved down the document is
/// unchanged and a block whose text changed is seen.
pub fn changed_blocks(old: &[Block], old_lines: &[String], new: &[Block], new_lines: &[String]) -> (Range<usize>, usize) {
    let same = |a: &Block, b: &Block| {
        let (ra, rb) = (a.lines.clone(), b.lines.clone());
        a.kind == b.kind && ra.len() == rb.len() && block_lines(old_lines, ra).eq(block_lines(new_lines, rb))
    };
    let limit = old.len().min(new.len());
    let prefix = (0..limit).take_while(|&i| same(&old[i], &new[i])).count();
    let suffix = (0..limit - prefix).take_while(|&i| same(&old[old.len() - 1 - i], &new[new.len() - 1 - i])).count();
    (prefix..old.len() - suffix, new.len() - prefix - suffix)
}

fn block_lines(lines: &[String], range: Range<usize>) -> impl Iterator<Item = &str> {
    let end = range.end.min(lines.len());
    lines[range.start.min(end)..end].iter().map(String::as_str)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub html: bool,
    pub link: Option<String>,
}

/// A piece of rendered text. `source` is the byte range of the run's own text in the block's
/// text, inside any markers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub struct Run {
    pub text: String,
    pub style: Style,
    pub source: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub enum Inline {
    Para(Vec<Run>),
    Heading(u8, Vec<Run>),
    Item { depth: u8, marker: String, task: Option<bool>, runs: Vec<Run> },
    Code { info: String, text: String },
    Quote(Vec<Run>),
    Rule,
    Table(String),
    Html(String),
    Metadata(String),
    Image { alt: String, path: String },
}

/// The inline content of one block's text. A soft break becomes a space, a hard break a newline,
/// inline HTML a run with `style.html`; a code run's source range excludes its backticks.
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub fn inline(block_text: &str, kind: &BlockKind) -> Vec<Inline> {
    match kind {
        BlockKind::Blank => Vec::new(),
        BlockKind::Rule => vec![Inline::Rule],
        BlockKind::Table => vec![Inline::Table(block_text.trim_end().to_string())],
        BlockKind::Html => vec![Inline::Html(block_text.trim_end().to_string())],
        BlockKind::CodeBlock { info } => vec![Inline::Code { info: info.clone(), text: plain_text(block_text) }],
        BlockKind::Metadata => vec![Inline::Metadata(plain_text(block_text))],
        BlockKind::Paragraph | BlockKind::Heading(_) | BlockKind::List | BlockKind::Quote => walk(block_text),
    }
}

/// Every text event of the block, joined: the content of a code or metadata block.
fn plain_text(block_text: &str) -> String {
    Parser::new_ext(block_text, OPTIONS)
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect()
}

/// The marker a list item's source starts with: `-`, `*`, `+`, or the number and its delimiter.
fn item_marker(source: &str) -> String {
    let mut chars = source.chars();
    match chars.next() {
        Some(c @ ('-' | '*' | '+')) => c.to_string(),
        Some(c) if c.is_ascii_digit() => {
            let digits = source.chars().take_while(char::is_ascii_digit).count();
            match source[digits..].chars().next() {
                Some(d @ ('.' | ')')) => format!("{}{d}", &source[..digits]),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

struct Walker {
    out: Vec<Inline>,
    runs: Vec<Run>,
    style: Style,
    saved: Vec<Style>,
    lists: usize,
    containers: usize,
    items: Vec<usize>,
    image: Option<(String, String)>,
    image_first: bool,
    para_image: Option<(String, String)>,
}

impl Walker {
    fn sink(&mut self) -> &mut Vec<Run> {
        match self.items.last() {
            Some(&i) => match &mut self.out[i] {
                Inline::Item { runs, .. } => runs,
                _ => unreachable!("items index only Item entries"),
            },
            None => &mut self.runs,
        }
    }

    fn push(&mut self, text: String, source: Range<usize>) {
        let style = self.style.clone();
        self.sink().push(Run { text, style, source });
    }
}

fn walk(src: &str) -> Vec<Inline> {
    let mut w = Walker { out: Vec::new(), runs: Vec::new(), style: Style::default(), saved: Vec::new(), lists: 0, containers: 0, items: Vec::new(), image: None, image_first: false, para_image: None };
    let mut heading = 1u8;
    for (event, range) in Parser::new_ext(src, OPTIONS).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    w.para_image = None;
                    if !w.sink().is_empty() {
                        w.push("\n".into(), range.start..range.start);
                    }
                }
                Tag::Heading { level, .. } => heading = level as u8,
                Tag::BlockQuote(_) => w.containers += 1,
                Tag::List(_) => {
                    w.containers += 1;
                    w.lists += 1;
                }
                Tag::Item => {
                    let depth = (w.lists.max(1) - 1) as u8;
                    let marker = item_marker(&src[range.start..]);
                    w.out.push(Inline::Item { depth, marker, task: None, runs: Vec::new() });
                    w.items.push(w.out.len() - 1);
                }
                Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. } | Tag::CodeBlock(_) => {
                    w.saved.push(w.style.clone());
                    match tag {
                        Tag::Strong => w.style.bold = true,
                        Tag::Emphasis => w.style.italic = true,
                        Tag::Strikethrough => w.style.strike = true,
                        Tag::Link { dest_url, .. } => w.style.link = Some(dest_url.to_string()),
                        _ => w.style.code = true,
                    }
                }
                Tag::Image { dest_url, .. } => {
                    w.image_first = w.sink().is_empty();
                    w.image = Some((String::new(), dest_url.to_string()));
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => {
                    if w.containers == 0 {
                        let runs = std::mem::take(&mut w.runs);
                        match w.para_image.take() {
                            Some((alt, path)) if runs.len() == 1 => w.out.push(Inline::Image { alt, path }),
                            _ => w.out.push(Inline::Para(runs)),
                        }
                    }
                }
                TagEnd::Heading(_) => {
                    if w.containers == 0 {
                        let runs = std::mem::take(&mut w.runs);
                        w.out.push(Inline::Heading(heading, runs));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    w.containers -= 1;
                    if w.containers == 0 {
                        let runs = std::mem::take(&mut w.runs);
                        w.out.push(Inline::Quote(runs));
                    }
                }
                TagEnd::List(_) => {
                    w.containers -= 1;
                    w.lists -= 1;
                }
                TagEnd::Item => {
                    w.items.pop();
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link | TagEnd::CodeBlock => {
                    w.style = w.saved.pop().unwrap_or_default();
                }
                TagEnd::Image => {
                    if let Some((alt, path)) = w.image.take() {
                        if w.containers == 0 && w.image_first {
                            w.para_image = Some((alt.clone(), path));
                        }
                        w.push(alt, range);
                    }
                }
                _ => {}
            },
            Event::Text(t) => match &mut w.image {
                Some((alt, _)) => alt.push_str(&t),
                None => w.push(t.to_string(), range),
            },
            Event::Code(t) => {
                let raw = &src[range.clone()];
                let ticks = raw.chars().take_while(|&c| c == '`').count();
                let inner_start = (range.start + ticks).min(range.end);
                let source = inner_start..range.end.saturating_sub(ticks).max(inner_start);
                match &mut w.image {
                    Some((alt, _)) => alt.push_str(&t),
                    None => {
                        let mut style = w.style.clone();
                        style.code = true;
                        w.sink().push(Run { text: t.to_string(), style, source });
                    }
                }
            }
            Event::SoftBreak => w.push(" ".into(), range),
            Event::HardBreak => w.push("\n".into(), range),
            Event::InlineHtml(h) | Event::Html(h) => {
                let mut style = w.style.clone();
                style.html = true;
                w.sink().push(Run { text: h.to_string(), style, source: range });
            }
            Event::TaskListMarker(done) => {
                if let Some(&i) = w.items.last() {
                    if let Inline::Item { task, .. } = &mut w.out[i] {
                        *task = Some(done);
                    }
                }
            }
            _ => {}
        }
    }
    w.out
}

/// A byte index into the rendered string (the runs' texts joined) to a byte offset in the block's
/// text: linear within a run's source, the next run's start between runs, and past the end the end
/// of the last run's source.
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub fn source_offset_at(runs: &[Run], rendered_byte: usize) -> usize {
    let mut at = 0;
    for run in runs {
        if rendered_byte < at + run.text.len() {
            return run.source.start + (rendered_byte - at).min(run.source.len());
        }
        at += run.text.len();
    }
    runs.last().map_or(0, |r| r.source.end)
}

/// A byte offset in the block's text (its lines joined with a newline) to a document position.
/// The blank lines at the end of the block belong to it but are not a place for the caret, so an
/// offset past the end clamps to the end of the block's last non-blank line, and a block of only
/// blank lines clamps to its first line at column 0.
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 6 on.
pub fn pos_of_offset(block: &Block, lines: &[String], byte: usize) -> edit::Pos {
    let first = block.lines.start.min(lines.len().saturating_sub(1));
    let end = block.lines.end.min(lines.len());
    let Some(last) = (first..end).rev().find(|&i| !lines[i].trim().is_empty()) else {
        return edit::Pos { line: first, ch: 0 };
    };
    let mut rest = byte;
    for (i, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        if rest <= line.len() {
            let mut cut = rest;
            while !line.is_char_boundary(cut) {
                cut -= 1;
            }
            return edit::Pos { line: i, ch: line[..cut].chars().count() };
        }
        rest -= line.len() + 1;
    }
    edit::Pos { line: last, ch: lines[last].chars().count() }
}

/// The parts of a list item line: its indent, its marker, and the byte where the content starts
/// after the marker and the space that follows it.
fn list_parts(line: &str) -> Option<(&str, &str, usize)> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let marker_len = match body.chars().next()? {
        '-' | '*' | '+' => 1,
        c if c.is_ascii_digit() => {
            let digits = body.chars().take_while(char::is_ascii_digit).count();
            if digits > 9 || !matches!(body[digits..].chars().next(), Some('.' | ')')) {
                return None;
            }
            digits + 1
        }
        _ => return None,
    };
    if !body[marker_len..].starts_with(' ') {
        return None;
    }
    Some((indent, &body[..marker_len], indent.len() + marker_len + 1))
}

/// A task box at the start of `s`: whether it is checked. It must end the string or be followed by
/// a space.
fn task_box(s: &str) -> Option<bool> {
    let checked = match s.get(..3)? {
        "[ ]" => false,
        "[x]" | "[X]" => true,
        _ => return None,
    };
    (s.len() == 3 || s[3..].starts_with(' ')).then_some(checked)
}

/// What Enter does on a list item line: the marker to insert on the next line (a number is
/// incremented, a task starts unchecked) and whether the item was empty, which ends the list.
pub fn list_continuation(line: &str) -> Option<(String, bool)> {
    let (indent, marker, content) = list_parts(line)?;
    let rest = &line[content..];
    let task = task_box(rest).is_some();
    let after = if task { rest[3..].trim() } else { rest.trim() };
    if after.is_empty() {
        return Some((format!("{indent}{marker} "), true));
    }
    let next = match marker.strip_suffix(['.', ')']) {
        Some(n) if n.chars().all(|c| c.is_ascii_digit()) => {
            let delim = &marker[n.len()..];
            format!("{}{delim}", n.parse::<u64>().map_or(n.to_string(), |v| (v + 1).to_string()))
        }
        _ => marker.to_string(),
    };
    Some((format!("{indent}{next} {}", if task { "[ ] " } else { "" }), false))
}

/// Flips `[ ]` and `[x]` on a task item line.
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
pub fn toggle_task(line: &str) -> Option<String> {
    let (_, _, content) = list_parts(line)?;
    let checked = task_box(&line[content..])?;
    let mark = if checked { "[ ]" } else { "[x]" };
    Some(format!("{}{mark}{}", &line[..content], &line[content + 3..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Marker,
    Heading,
    Emphasis,
    Code,
    Link,
    Quote,
}

/// A light colourer for one line of raw markdown: heading, list, task, fence and quote markers,
/// emphasis and code pairs, links. Byte ranges that may leave gaps; the painter fills the gaps in
/// the text colour.
pub fn raw_marks(line: &str) -> Vec<(Range<usize>, Mark)> {
    let mut marks = Vec::new();
    let body = line.trim_start();
    let indent = line.len() - body.len();
    if body.starts_with("```") || body.starts_with("~~~") {
        marks.push((indent..line.len(), Mark::Marker));
        return marks;
    }
    let hashes = body.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && (body.len() == hashes || body[hashes..].starts_with(' ')) {
        marks.push((indent..indent + hashes, Mark::Marker));
        if body.len() > hashes + 1 {
            marks.push((indent + hashes + 1..line.len(), Mark::Heading));
        }
        return marks;
    }
    let mut from = 0;
    if body.starts_with('>') {
        marks.push((indent..indent + 1, Mark::Quote));
        from = indent + 1;
    } else if let Some((_, marker, content)) = list_parts(line) {
        let mut end = indent + marker.len();
        if task_box(&line[content..]).is_some() {
            end = content + 3;
        }
        marks.push((indent..end, Mark::Marker));
        from = end;
    }
    scan_inline(line, from, &mut marks);
    marks
}

/// The inline pass of `raw_marks`: code spans, emphasis pairs and links from `from` on.
fn scan_inline(line: &str, from: usize, marks: &mut Vec<(Range<usize>, Mark)>) {
    let bytes = line.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let n = bytes[i..].iter().take_while(|&&b| b == b'`').count();
                let ticks = &line[i..i + n];
                if let Some(close) = line[i + n..].find(ticks) {
                    let end = i + n + close + n;
                    marks.push((i..end, Mark::Code));
                    i = end;
                    continue;
                }
                i += n;
            }
            b'*' | b'_' => {
                let c = bytes[i];
                let n = bytes[i..].iter().take_while(|&&b| b == c).count().min(2);
                let word_before = i > 0 && c == b'_' && (bytes[i - 1].is_ascii_alphanumeric());
                let delim = &line[i..i + n];
                if !word_before {
                    if let Some(close) = line[i + n..].find(delim).filter(|&close| close > 0) {
                        let end = i + n + close + n;
                        marks.push((i..end, Mark::Emphasis));
                        i = end;
                        continue;
                    }
                }
                i += n;
            }
            b'[' => {
                let link = line[i..].find("](").and_then(|mid| line[i + mid..].find(')').map(|close| i + mid + close + 1));
                if let Some(end) = link {
                    marks.push((i..end, Mark::Link));
                    i = end;
                    continue;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn lines(s: &str) -> Vec<String> {
        s.split('\n').map(String::from).collect()
    }

    fn para_runs(text: &str) -> Vec<Run> {
        match &inline(text, &BlockKind::Paragraph)[0] {
            Inline::Para(r) => r.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_line_lands_in_exactly_one_block_in_order() {
        let doc = lines("# Title\n\nA paragraph\nover two lines.\n\n- one\n- two\n  - nested\n\n```rust\nfn x() {}\n```\n\n> quoted\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<div>raw</div>\n");
        let blocks = split(&doc);
        let kinds: Vec<&BlockKind> = blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(kinds, vec![&BlockKind::Heading(1), &BlockKind::Paragraph, &BlockKind::List, &BlockKind::CodeBlock { info: "rust".into() }, &BlockKind::Quote, &BlockKind::Rule, &BlockKind::Table, &BlockKind::Html]);
        let mut covered = 0;
        for b in &blocks {
            assert_eq!(b.lines.start, covered, "blocks are contiguous");
            covered = b.lines.end;
        }
        assert_eq!(covered, doc.len(), "and cover the document; the final empty line stays in the last block");
        assert_eq!(blocks[0].lines, 0..2, "the blank line after the heading belongs to it");
        assert_eq!(block_at(&blocks, 7), 2, "the nested item is in the list block");
        assert_eq!(block_at(&blocks, 999), blocks.len() - 1, "past the end: the last block");
        assert_eq!(split(&[]).len(), 0);
        assert_eq!(split(&lines("")), vec![Block { lines: 0..1, kind: BlockKind::Blank }]);
        assert_eq!(
            split(&lines("\n\n# H")),
            vec![Block { lines: 0..2, kind: BlockKind::Blank }, Block { lines: 2..3, kind: BlockKind::Heading(1) }],
            "leading blank lines are a Blank block"
        );
        assert_eq!(
            split(&lines("---\ntitle: x\n---\n\n# H")),
            vec![Block { lines: 0..4, kind: BlockKind::Metadata }, Block { lines: 4..5, kind: BlockKind::Heading(1) }],
            "front matter is a Metadata block"
        );
    }

    #[test]
    fn inline_runs_carry_styles_and_source_offsets() {
        let runs = para_runs("a **bold** and `code` [link](https://x.y) ~~gone~~");
        let texts: Vec<(&str, bool, bool, bool, Option<&str>)> = runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.code, r.style.strike, r.style.link.as_deref())).collect();
        assert_eq!(texts, vec![("a ", false, false, false, None), ("bold", true, false, false, None), (" and ", false, false, false, None), ("code", false, true, false, None), (" ", false, false, false, None), ("link", false, false, false, Some("https://x.y")), (" ", false, false, false, None), ("gone", false, false, true, None)]);
        assert_eq!(runs[1].source, 4..8, "the source range is the text inside the markers");
        assert_eq!(runs[3].source, 16..20, "a code run excludes the backticks");
        assert_eq!(runs[5].source, 23..27);
        let soft: Vec<String> = para_runs("a\nb").iter().map(|r| r.text.clone()).collect();
        assert_eq!(soft, vec!["a", " ", "b"], "a soft break renders as a space");
        let hard: Vec<String> = para_runs("a  \nb").iter().map(|r| r.text.clone()).collect();
        assert_eq!(hard, vec!["a", "\n", "b"], "a hard break renders as a newline");
        let html = para_runs("x <b>y</b>");
        assert!(html.iter().any(|r| r.style.html && r.text == "<b>"));
    }

    #[test]
    fn lists_yield_items_with_depth_markers_and_tasks() {
        let items = inline("1. first\n2. second\n   - [x] done\n   - [ ] todo", &BlockKind::List);
        let got: Vec<(u8, &str, Option<bool>, String)> = items.iter().map(|i| match i { Inline::Item { depth, marker, task, runs } => (*depth, marker.as_str(), *task, runs.iter().map(|r| r.text.clone()).collect()), other => panic!("{other:?}") }).collect();
        assert_eq!(got, vec![(0, "1.", None, "first".into()), (0, "2.", None, "second".into()), (1, "-", Some(true), "done".into()), (1, "-", Some(false), "todo".into())]);
        let loose = inline("- [x] a\n\n- [ ] b", &BlockKind::List);
        let tasks: Vec<Option<bool>> = loose.iter().map(|i| match i { Inline::Item { task, .. } => *task, other => panic!("{other:?}") }).collect();
        assert_eq!(tasks, vec![Some(true), Some(false)], "a loose list keeps its tasks");
    }

    #[test]
    fn code_images_tables_and_html_pass_through() {
        assert_eq!(inline("```py\nprint(1)\n```", &BlockKind::CodeBlock { info: "py".into() }), vec![Inline::Code { info: "py".into(), text: "print(1)\n".into() }]);
        assert_eq!(inline("![alt text](img/a.png)", &BlockKind::Paragraph), vec![Inline::Image { alt: "alt text".into(), path: "img/a.png".into() }]);
        assert!(matches!(&inline("see ![a](b.png) here", &BlockKind::Paragraph)[0], Inline::Para(_)), "an image inside text is text");
        assert_eq!(inline("| a |\n|---|", &BlockKind::Table), vec![Inline::Table("| a |\n|---|".into())]);
        assert_eq!(inline("<b>x</b>", &BlockKind::Html), vec![Inline::Html("<b>x</b>".into())]);
        assert_eq!(inline("---\ntitle: x\n---", &BlockKind::Metadata), vec![Inline::Metadata("title: x\n".into())]);
        assert_eq!(inline("---", &BlockKind::Rule), vec![Inline::Rule]);
        assert!(matches!(&inline("> a\n> b", &BlockKind::Quote)[0], Inline::Quote(r) if r.len() == 3));
        assert!(matches!(&inline("## h", &BlockKind::Heading(2))[0], Inline::Heading(2, _)));
    }

    #[test]
    fn enter_continues_a_list_and_ends_an_empty_item() {
        assert_eq!(list_continuation("- item"), Some(("- ".into(), false)));
        assert_eq!(list_continuation("  * item"), Some(("  * ".into(), false)));
        assert_eq!(list_continuation("3. third"), Some(("4. ".into(), false)));
        assert_eq!(list_continuation("- [x] done"), Some(("- [ ] ".into(), false)));
        assert_eq!(list_continuation("- "), Some(("- ".into(), true)), "an empty item: Enter ends the list");
        assert_eq!(list_continuation("plain"), None);
        assert_eq!(toggle_task("- [ ] todo"), Some("- [x] todo".into()));
        assert_eq!(toggle_task("  1. [x] done"), Some("  1. [ ] done".into()));
        assert_eq!(toggle_task("- plain"), None);
    }

    #[test]
    fn raw_marks_colour_the_syntax_a_line_shows() {
        assert_eq!(raw_marks("## Head"), vec![(0..2, Mark::Marker), (3..7, Mark::Heading)]);
        assert_eq!(raw_marks("- [ ] a **b** `c`"), vec![(0..5, Mark::Marker), (8..13, Mark::Emphasis), (14..17, Mark::Code)]);
        assert_eq!(raw_marks("> q"), vec![(0..1, Mark::Quote)]);
        assert_eq!(raw_marks("see [x](y)"), vec![(4..10, Mark::Link)]);
        assert_eq!(raw_marks("```rust"), vec![(0..7, Mark::Marker)]);
        assert!(raw_marks("plain words").is_empty());
    }

    #[test]
    fn changed_blocks_names_the_edited_range() {
        let (la, lb, lc, ld) = (lines("# T\n\npara\n\n- x\n"), lines("# T\n\npara two\n\n- x\n"), lines("# T\n\npara\n\nnew para\n\n- x\n"), lines("# T\n\n- x\n"));
        let (a, b, c, d) = (split(&la), split(&lb), split(&lc), split(&ld));
        assert_eq!(changed_blocks(&a, &la, &b, &lb), (1..2, 1), "one block's text changed");
        assert_eq!(changed_blocks(&a, &la, &c, &lc), (2..2, 1), "a block inserted before the list");
        assert_eq!(changed_blocks(&a, &la, &d, &ld), (1..2, 0), "a block removed");
        assert_eq!(changed_blocks(&a, &la, &a, &la), (a.len()..a.len(), 0), "nothing changed");
        assert_eq!(changed_blocks(&[], &[], &a, &la), (0..0, a.len()));
    }

    #[test]
    fn rendered_bytes_map_back_to_the_source_and_to_positions() {
        let runs = para_runs("a **bold** c");
        // Rendered text "a bold c": bytes 0 'a', 2..6 "bold", 7 'c'.
        assert_eq!(source_offset_at(&runs, 0), 0);
        assert_eq!(source_offset_at(&runs, 2), 4, "inside the bold run: past the opening **");
        assert_eq!(source_offset_at(&runs, 5), 7);
        assert_eq!(source_offset_at(&runs, 7), 11, "the c after the closing **");
        assert_eq!(source_offset_at(&runs, 99), 12, "past the end: the block's end");
        let doc = lines("# T\n\nab\ncd\n");
        let blocks = split(&doc);
        assert_eq!(pos_of_offset(&blocks[1], &doc, 0), edit::Pos { line: 2, ch: 0 });
        assert_eq!(pos_of_offset(&blocks[1], &doc, 3), edit::Pos { line: 3, ch: 0 }, "past the first line's newline");
        assert_eq!(pos_of_offset(&blocks[1], &doc, 99), edit::Pos { line: 3, ch: 2 }, "clamped to the end of the last non-blank line");
        let blank = lines("\n\n# H");
        assert_eq!(pos_of_offset(&split(&blank)[0], &blank, 99), edit::Pos { line: 0, ch: 0 }, "a blank-only block clamps to its first line");
    }

    #[test]
    #[ignore] // Timing: run by hand with `-- --ignored`; the gate is 4 ms in a release build.
    fn a_megabyte_splits_in_time() {
        let unit = "# Heading\n\nSome paragraph text that goes on for a while to look like prose.\n\n- a list item\n- another with **bold**\n\n```rust\nfn x() {}\n```\n\n";
        let mut text = String::new();
        while text.len() < 1 << 20 {
            text.push_str(unit);
        }
        let doc = lines(&text);
        let t = std::time::Instant::now();
        let blocks = split(&doc);
        let took = t.elapsed();
        println!("split of {} bytes into {} blocks took {took:?}", text.len(), blocks.len());
        assert!(blocks.len() > 1000);
        assert!(took < Duration::from_millis(4), "split took {took:?}");
    }
}
