//! The inline reveal (`docs/superpowers/specs/2026-10-30-inline-reveal-design.md`): how the
//! rendered view draws the block holding the caret. The block stays rendered; the inline element
//! under the caret (or under either end of the selection, or a find hit) shows its source, a
//! heading shows its hashes, and the markers of list items and quotes never show. Pure: a block's
//! text and element tree (`md_blocks::elements`) in, a `Display` out, with the map between source
//! offsets (bytes of the block's text) and display bytes both ways, and the caret helpers that keep
//! the caret off hidden markers.

use crate::md_blocks::{self, MarkerClass, Node, NodeKind, Style};
use std::ops::Range;

/// Where the user is in a block, in bytes of its text: the caret, the selection's ends, a find
/// hit's ends (`offsets`, which decide what is revealed), and the ranges painted over the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    pub offsets: Vec<usize>,
    pub selection: Option<Range<usize>>,
    /// The find hits in the block, `true` for the current one.
    pub hits: Vec<(Range<usize>, bool)>,
    /// The input method's uncommitted text.
    pub marked: Option<Range<usize>>,
}

/// What a segment of the source shows as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// The source bytes themselves.
    Verbatim,
    /// Other text: a run as the parser renders it (an entity decoded, a soft break as a space).
    Rendered(String),
    /// Nothing.
    Hidden,
}

/// A piece of the source with what it shows as and the style it is drawn in. `marker` says it is
/// syntax (a node's bytes no child covers) rather than text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub source: Range<usize>,
    pub shown: Shown,
    pub style: Style,
    pub marker: bool,
}

/// One displayed text: the segments tiling `covers` (a range of the block's text) in order, the
/// text they show, each segment's range in that text, and the revealed links' addresses as display
/// ranges with their targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub segments: Vec<Segment>,
    pub display: Vec<Range<usize>>,
    pub text: String,
    pub covers: Range<usize>,
    pub links: Vec<(Range<usize>, String)>,
}

/// A caret move's direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

fn is_leaf(n: &Node) -> bool {
    n.children.is_empty() && matches!(n.kind, NodeKind::Text | NodeKind::SoftBreak | NodeKind::HardBreak | NodeKind::Html | NodeKind::Other)
}

fn contains(span: &Range<usize>, o: usize) -> bool {
    span.start <= o && o <= span.end
}

/// The style a node gives the text inside it, on top of its parent's.
fn styled(kind: &NodeKind, parent: &Style) -> Style {
    let mut s = parent.clone();
    match kind {
        NodeKind::Strong => s.bold = true,
        NodeKind::Emphasis => s.italic = true,
        NodeKind::Strike => s.strike = true,
        NodeKind::Code => s.code = true,
        NodeKind::Html => s.html = true,
        NodeKind::Link { url, .. } => s.link = Some(url.clone()),
        _ => {}
    }
    s
}

/// The node at `path` (indices from the top-level nodes).
pub fn node_at<'a>(nodes: &'a [Node], path: &[usize]) -> Option<&'a Node> {
    let (first, rest) = path.split_first()?;
    let n = nodes.get(*first)?;
    if rest.is_empty() {
        Some(n)
    } else {
        node_at(&n.children, rest)
    }
}

/// The index paths of every list item, in document order (the order the rendered view's item
/// pieces have).
pub fn item_paths(nodes: &[Node]) -> Vec<Vec<usize>> {
    fn walk(nodes: &[Node], path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, n) in nodes.iter().enumerate() {
            path.push(i);
            if n.kind == NodeKind::Item {
                out.push(path.clone());
            }
            walk(&n.children, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(nodes, &mut Vec::new(), &mut out);
    out
}

/// Where an item's own text ends: at its first nested list, else at its end.
pub fn item_end(item: &Node) -> usize {
    item.children.iter().find(|c| c.kind == NodeKind::List).map_or(item.span.end, |l| l.span.start)
}

/// The item whose own text holds `offset`: the last item starting at or before it whose own range
/// reaches it.
pub fn item_at(nodes: &[Node], offset: usize) -> Option<Vec<usize>> {
    item_paths(nodes).into_iter().rev().find(|p| node_at(nodes, p).is_some_and(|n| n.span.start <= offset && offset <= item_end(n)))
}

/// The outermost inline element whose span holds `o`, ends included.
fn outermost_inline(nodes: &[Node], o: usize) -> Option<&Node> {
    for n in nodes {
        if !contains(&n.span, o) || is_leaf(n) {
            continue;
        }
        if n.kind.is_inline() {
            return Some(n);
        }
        if let Some(found) = outermost_inline(&n.children, o) {
            return Some(found);
        }
    }
    None
}

/// The deepest container (not inline, not a leaf) whose span holds `o`.
fn container(nodes: &[Node], o: usize) -> Option<&Node> {
    for n in nodes {
        if is_leaf(n) || n.kind.is_inline() || !contains(&n.span, o) {
            continue;
        }
        return container(&n.children, o).or(Some(n));
    }
    None
}

/// The plain text around `o` outside any inline element: the run of leaves and revealable markers
/// (an escape's backslash) of its container that holds `o`, so an entity or an escape there shows
/// as written. `None` when `o` is on an element or a hidden marker.
fn plain_stretch(src: &str, nodes: &[Node], o: usize) -> Option<Range<usize>> {
    let c = container(nodes, o)?;
    let mut parts: Vec<(Range<usize>, bool)> = Vec::new();
    let mut cursor = c.span.start;
    for child in &c.children {
        if cursor < child.span.start {
            parts.push((cursor..child.span.start, md_blocks::marker_class(src, cursor..child.span.start) == MarkerClass::Revealable));
        }
        parts.push((child.span.clone(), is_leaf(child) && child.kind != NodeKind::Other));
        cursor = cursor.max(child.span.end);
    }
    if cursor < c.span.end {
        parts.push((cursor..c.span.end, false));
    }
    let k = parts.iter().position(|(r, ok)| *ok && contains(r, o))?;
    let (mut l, mut r) = (k, k);
    while l > 0 && parts[l - 1].1 {
        l -= 1;
    }
    while r + 1 < parts.len() && parts[r + 1].1 {
        r += 1;
    }
    Some(parts[l].0.start..parts[r].0.end)
}

/// The gaps of a node between and around its children.
fn gaps(n: &Node) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut cursor = n.span.start;
    for c in &n.children {
        if cursor < c.span.start {
            out.push(cursor..c.span.start);
        }
        cursor = cursor.max(c.span.end);
    }
    if cursor < n.span.end {
        out.push(cursor..n.span.end);
    }
    out
}

/// The source ranges shown verbatim for these focus offsets (spec section 4, rule 1): per offset
/// the outermost inline element holding it, else the plain text around it; and a heading's own
/// markers whenever any offset is given.
pub fn revealed(src: &str, nodes: &[Node], offsets: &[usize]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for &o in offsets {
        match outermost_inline(nodes, o) {
            Some(n) => out.push(n.span.clone()),
            None => out.extend(plain_stretch(src, nodes, o)),
        }
    }
    if !offsets.is_empty() {
        for n in nodes.iter().filter(|n| matches!(n.kind, NodeKind::Heading(_))) {
            out.extend(gaps(n));
        }
    }
    out
}

struct Builder<'a> {
    src: &'a str,
    reveal: Vec<Range<usize>>,
    segments: Vec<Segment>,
}

impl Builder<'_> {
    fn shown(&self, r: &Range<usize>) -> bool {
        self.reveal.iter().any(|v| v.start <= r.start && r.end <= v.end)
    }

    fn push(&mut self, source: Range<usize>, shown: Shown, style: Style, marker: bool) {
        if source.start >= source.end {
            return;
        }
        let shown = match shown {
            Shown::Rendered(t) if t.is_empty() => Shown::Hidden,
            other => other,
        };
        self.segments.push(Segment { source, shown, style, marker });
    }

    fn gap(&mut self, r: Range<usize>, between: bool, style: &Style) {
        if r.start >= r.end {
            return;
        }
        let shown = match md_blocks::marker_class(self.src, r.clone()) {
            MarkerClass::Hidden if between && self.src[r.clone()].contains('\n') => Shown::Rendered("\n".into()),
            MarkerClass::Hidden => Shown::Hidden,
            MarkerClass::Revealable if self.shown(&r) => Shown::Verbatim,
            MarkerClass::Revealable => Shown::Hidden,
        };
        self.push(r, shown, style.clone(), true);
    }

    /// Emits `n`'s segments up to `end`, a nested list ending an item's own text when `cut`.
    fn node(&mut self, n: &Node, parent: &Style, cut: bool) {
        if is_leaf(n) {
            let source = &self.src[n.span.clone()];
            let shown = match n.kind {
                NodeKind::SoftBreak => Shown::Rendered(" ".into()),
                NodeKind::Other => Shown::Hidden,
                _ if self.shown(&n.span) || n.text == source => Shown::Verbatim,
                _ => Shown::Rendered(n.text.clone()),
            };
            self.push(n.span.clone(), shown, styled(&n.kind, parent), false);
            return;
        }
        let style = styled(&n.kind, parent);
        let kids: Vec<&Node> = n.children.iter().take_while(|c| !(cut && c.kind == NodeKind::List)).collect();
        let end = if cut { item_end(n) } else { n.span.end };
        let mut cursor = n.span.start;
        for (i, c) in kids.iter().enumerate() {
            self.gap(cursor..c.span.start, i > 0, &style);
            self.node(c, &style, false);
            cursor = cursor.max(c.span.end);
        }
        self.gap(cursor..end.max(cursor), false, &style);
    }

    fn finish(mut self, covers: Range<usize>, nodes: &[Node]) -> Display {
        // Bytes no node holds (blank lines after the block) show as nothing.
        let mut tiled = Vec::with_capacity(self.segments.len());
        let mut cursor = covers.start;
        for s in self.segments.drain(..) {
            if cursor < s.source.start {
                tiled.push(Segment { source: cursor..s.source.start, shown: Shown::Hidden, style: Style::default(), marker: false });
            }
            cursor = s.source.end;
            tiled.push(s);
        }
        if cursor < covers.end {
            tiled.push(Segment { source: cursor..covers.end, shown: Shown::Hidden, style: Style::default(), marker: false });
        }
        let mut text = String::new();
        let mut display = Vec::with_capacity(tiled.len());
        for s in &tiled {
            let start = text.len();
            match &s.shown {
                Shown::Verbatim => text.push_str(&self.src[s.source.clone()]),
                Shown::Rendered(t) => text.push_str(t),
                Shown::Hidden => {}
            }
            display.push(start..text.len());
        }
        let mut d = Display { segments: tiled, display, text, covers, links: Vec::new() };
        let mut links = Vec::new();
        collect_links(nodes, &self.reveal, &mut links);
        d.links = links.into_iter().filter(|(r, _)| d.covers.start <= r.start && r.end <= d.covers.end).map(|(r, url)| (d.display_of(r.start)..d.display_of(r.end), url)).collect();
        d
    }
}

fn collect_links(nodes: &[Node], reveal: &[Range<usize>], out: &mut Vec<(Range<usize>, String)>) {
    for n in nodes {
        if let NodeKind::Link { dest: Some(dest), url } = &n.kind {
            if reveal.iter().any(|v| v.start <= dest.start && dest.end <= v.end) {
                out.push((dest.clone(), url.clone()));
            }
        }
        collect_links(&n.children, reveal, out);
    }
}

impl Display {
    /// The display of a paragraph, heading or quote block: all of `text`.
    pub fn new(text: &str, nodes: &[Node], offsets: &[usize]) -> Display {
        let mut b = Builder { src: text, reveal: revealed(text, nodes, offsets), segments: Vec::new() };
        for n in nodes {
            b.node(n, &Style::default(), false);
        }
        b.finish(0..text.len(), nodes)
    }

    /// The display of one list item's own text (up to its nested list), the item at `path`.
    pub fn for_item(text: &str, nodes: &[Node], path: &[usize], offsets: &[usize]) -> Option<Display> {
        let item = node_at(nodes, path)?;
        let mut b = Builder { src: text, reveal: revealed(text, nodes, offsets), segments: Vec::new() };
        b.node(item, &Style::default(), true);
        Some(b.finish(item.span.start..item_end(item), nodes))
    }

    /// Every byte of `covers` as written: an image paragraph's definition under its picture.
    pub fn verbatim(text: &str, covers: Range<usize>) -> Display {
        let seg = Segment { source: covers.clone(), shown: Shown::Verbatim, style: Style::default(), marker: false };
        Display { display: vec![0..covers.len()], text: text[covers.clone()].to_string(), segments: vec![seg], covers, links: Vec::new() }
    }

    /// The display byte where source offset `o` is drawn: exact in verbatim text, the start or end
    /// of rendered text, the end of a hidden segment (so an offset in a hidden marker is drawn
    /// where its text starts). Monotone.
    pub fn display_of(&self, o: usize) -> usize {
        if o <= self.covers.start {
            return 0;
        }
        let i = self.segments.partition_point(|s| s.source.end <= o);
        let Some(s) = self.segments.get(i) else { return self.text.len() };
        let d = &self.display[i];
        match s.shown {
            Shown::Verbatim => d.start + (o - s.source.start).min(d.len()),
            Shown::Rendered(_) if o == s.source.start => d.start,
            Shown::Rendered(_) | Shown::Hidden => d.end,
        }
    }

    /// The source offset a display byte stands for: exact in verbatim text, the start or end of
    /// rendered text, and never inside a hidden marker (a line break shown between two paragraphs
    /// stands for the end of the text before it). Monotone.
    pub fn source_of(&self, d: usize) -> usize {
        let i = self.display.partition_point(|r| r.end <= d);
        let last_text = |before: usize| self.segments[..before].iter().rev().find(|s| s.shown != Shown::Hidden && !s.marker).map(|s| s.source.end);
        let Some(s) = self.segments.get(i) else { return last_text(self.segments.len()).unwrap_or(self.covers.start) };
        let r = &self.display[i];
        match &s.shown {
            Shown::Verbatim => s.source.start + (d - r.start),
            Shown::Rendered(_) if s.marker && d == r.start => last_text(i).unwrap_or(s.source.start),
            Shown::Rendered(_) if d == r.start => s.source.start,
            _ => s.source.end,
        }
    }

    /// A source range as a display range.
    pub fn range(&self, r: &Range<usize>) -> Range<usize> {
        self.display_of(r.start)..self.display_of(r.end.max(r.start))
    }
}

/// Whether offset `o` is a place the caret cannot be seen: inside a hidden marker, not on its line
/// break (the end of the text before it, which is drawn).
fn dead(text: &str, markers: &[md_blocks::Marker], o: usize) -> Option<Range<usize>> {
    if text.as_bytes().get(o) == Some(&b'\n') {
        return None;
    }
    markers.iter().find(|m| m.class == MarkerClass::Hidden && m.range.start <= o && o < m.range.end).map(|m| m.range.clone())
}

/// A caret offset moved off hidden markers, onward in `dir`: Right (and Down) to the marker's end,
/// Left (and Up) to the offset before the marker (the end of the line above), or onward when there
/// is none (the block's first line).
pub fn snap(text: &str, markers: &[md_blocks::Marker], mut o: usize, dir: Dir) -> usize {
    let mut left = matches!(dir, Dir::Left | Dir::Up);
    for _ in 0..markers.len() + 2 {
        let Some(m) = dead(text, markers, o) else { return o };
        if left && m.start > 0 {
            o = m.start - 1;
        } else {
            left = false;
            o = m.end;
        }
    }
    o
}

/// Backspace at the first visible offset of a list item or of a paragraph in a quote: the source
/// range to replace and its new text, which takes the item's marker away (the line becomes text)
/// or every line's `>` of that paragraph (it leaves the quote), as one edit. `None` anywhere else.
pub fn unmark(text: &str, nodes: &[Node], o: usize) -> Option<(Range<usize>, String)> {
    fn first_text(n: &Node) -> Option<usize> {
        if is_leaf(n) {
            return Some(n.span.start);
        }
        n.children.first().and_then(first_text)
    }
    for path in item_paths(nodes).iter().rev() {
        let item = node_at(nodes, path)?;
        if item.children.first().and_then(first_text) == Some(o) && item.span.start < o {
            return Some((item.span.start..o, String::new()));
        }
    }
    let quote = nodes.iter().find(|n| n.kind == NodeKind::Quote)?;
    let para = quote.children.iter().find(|p| first_text(p) == Some(o))?;
    let from = text[..para.span.start].rfind('\n').map_or(0, |i| i + 1);
    // A paragraph's span holds the line break after it, which stays.
    let to = from + text[from..para.span.end].trim_end_matches('\n').len();
    let rewritten: Vec<String> = text[from..to]
        .split('\n')
        .map(|line| {
            let lead = line.len() - line.trim_start().len();
            match line[lead..].strip_prefix('>') {
                Some(rest) => rest.strip_prefix(' ').unwrap_or(rest).to_string(),
                None => line.to_string(),
            }
        })
        .collect();
    Some((from..to, rewritten.join("\n")))
}

/// The row of `rows` (display byte ranges, one per displayed row) holding display byte `at`.
fn row_of(rows: &[Range<usize>], at: usize) -> usize {
    rows.iter().rposition(|r| r.start <= at).unwrap_or(0)
}

/// Up or Down by displayed row: the display byte on the next row in `dir` whose x (`x_of`) is
/// nearest `goal_x`, or `None` on the first or last row (the move leaves the block). Only char
/// boundaries of `text` are candidates.
pub fn vertical(text: &str, rows: &[Range<usize>], x_of: impl Fn(usize) -> f32, at: usize, dir: Dir, goal_x: f32) -> Option<usize> {
    let r = row_of(rows, at);
    let target = match dir {
        Dir::Up => r.checked_sub(1)?,
        Dir::Down => Some(r + 1).filter(|&t| t < rows.len())?,
        Dir::Left | Dir::Right => return None,
    };
    let row = &rows[target];
    (row.start..=row.end.min(text.len())).filter(|&b| text.is_char_boundary(b)).min_by(|&a, &b| (x_of(a) - goal_x).abs().total_cmp(&(x_of(b) - goal_x).abs()))
}

/// The display bytes where the row holding `at` starts and ends.
pub fn row_edges(rows: &[Range<usize>], at: usize) -> (usize, usize) {
    rows.get(row_of(rows, at)).map_or((at, at), |r| (r.start, r.end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md_blocks::{elements, markers, Marker};
    use std::time::{Duration, Instant};

    /// The display of `src` with the caret (or a selection's two ends) at `offsets`: the caret's
    /// item when the block is a list.
    fn display(src: &str, offsets: &[usize]) -> Display {
        let nodes = elements(src);
        if nodes.first().is_some_and(|n| n.kind == NodeKind::List) {
            let path = item_at(&nodes, offsets[0]).expect("an item holds the caret");
            return Display::for_item(src, &nodes, &path, offsets).unwrap();
        }
        Display::new(src, &nodes, offsets)
    }

    fn marks(src: &str) -> Vec<Marker> {
        markers(src, &elements(src))
    }

    #[test]
    fn nothing_is_revealed_with_the_caret_outside_every_element_and_the_map_is_monotone() {
        let src = "a **b** c\nd";
        let d = display(src, &[0]);
        assert_eq!(d.text, "a b c d");
        for i in 0..src.len() {
            assert!(d.display_of(i) <= d.display_of(i + 1), "display_of at {i}");
        }
        for i in 0..d.text.len() {
            assert!(d.source_of(i) <= d.source_of(i + 1), "source_of at {i}");
        }
        assert_eq!(d.source_of(d.display_of(4)), 4, "a verbatim byte round-trips");
    }

    #[test]
    fn the_caret_in_a_strong_shows_its_markers_bold_until_one_past_the_closing_one() {
        let d = display("a **b** c", &[4]);
        assert_eq!(d.text, "a **b** c");
        let markers: Vec<&Segment> = d.segments.iter().filter(|s| s.marker && s.shown == Shown::Verbatim).collect();
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().all(|s| s.style.bold));
        assert_eq!(display("a **b** c", &[2]).text, "a **b** c", "just before the opening marker: revealed");
        assert_eq!(display("a **b** c", &[7]).text, "a **b** c", "just after the closing marker: still revealed");
        assert_eq!(display("a **b** c", &[8]).text, "a b c", "one further: hidden");
    }

    #[test]
    fn a_nested_element_reveals_with_its_outer_one_and_plain_text_shows_its_escapes() {
        assert_eq!(display("**a _b_ c**", &[5]).text, "**a _b_ c**");
        assert_eq!(display("x \\* y &amp; z", &[3]).text, "x \\* y &amp; z");
        assert_eq!(display("x \\* y &amp; z", &[0]).text, "x \\* y &amp; z", "the plain run is the whole paragraph");
        assert_eq!(display("x \\* y **&amp;**", &[0]).text, "x \\* y &", "an entity in an element away from the caret is rendered");
        assert_eq!(display("[e](https://x) f", &[1]).text, "[e](https://x) f");
        assert_eq!(display("`` a ` b `` c", &[3]).text, "`` a ` b `` c");
        assert_eq!(display("`` a ` b `` c", &[13]).text, "a ` b c");
    }

    #[test]
    fn a_heading_shows_its_hashes_anywhere_on_it_and_an_item_never_shows_its_bullet() {
        assert_eq!(display("## Title", &[8]).text, "## Title");
        assert_eq!(display("## Title", &[]).text, "Title");
        let d = display("- one\n- two", &[2]);
        assert_eq!(d.text, "one", "only the caret's item");
        assert_eq!(d.display_of(0), 0, "an offset in the hidden marker is drawn where the text starts");
        assert_eq!(d.source_of(0), 2, "and the display's start is the first visible offset");
        assert_eq!(d.source_of(3), 5);
        let d = display("- a\n  - b\n- c", &[2]);
        assert_eq!(d.text, "a", "the nested list is not the item's own text");
        assert_eq!(display("- [ ] **t**", &[7]).text, "**t**");
        let q = display("> one\n> two", &[3]);
        assert_eq!(q.text, "one two", "the continuation's line break is a space and its > is hidden");
        assert_eq!(q.source_of(4), 8);
        let two = display("> a\n>\n> b", &[2]);
        assert_eq!(two.text, "a\nb", "a quote's paragraphs are lines");
        assert_eq!(two.source_of(1), 3, "the line break stands for the end of the text before it");
    }

    #[test]
    fn a_selection_reveals_both_ends_and_nothing_between() {
        assert_eq!(display("**a** b `c` d ~~e~~", &[2, 17]).text, "**a** b c d ~~e~~");
    }

    #[test]
    fn a_revealed_link_offers_its_address_as_a_display_range() {
        let src = "[e](https://x) [f](https://y)";
        let d = display(src, &[1]);
        assert_eq!(d.links, vec![(4..13, "https://x".to_string())]);
        assert_eq!(&d.text[4..13], "https://x");
        assert!(display(src, &[]).links.is_empty());
    }

    #[test]
    fn snap_steps_over_hidden_markers_and_unmark_takes_a_prefix_only_at_the_text_start() {
        let src = "- one\n- two";
        let m = marks(src);
        assert_eq!(snap(src, &m, 6, Dir::Right), 8, "Right onto two's marker lands after it");
        assert_eq!(snap(src, &m, 7, Dir::Left), 5, "Left into the marker goes to the line above's end");
        assert_eq!(snap(src, &m, 0, Dir::Left), 2, "with no line above, onward");
        assert_eq!(snap(src, &m, 5, Dir::Right), 5, "a line's end is alive");
        let nodes = elements(src);
        assert_eq!(unmark(src, &nodes, 8), Some((6..8, String::new())));
        assert_eq!(unmark(src, &nodes, 9), None);
        let boxed = "- [ ] a";
        assert_eq!(unmark(boxed, &elements(boxed), 6), Some((0..6, String::new())));
        let q = "> q1\n> q2\n\nafter";
        assert_eq!(unmark(q, &elements(q), 2), Some((0..9, "q1\nq2".to_string())), "the paragraph leaves the quote");
        let h = "## h";
        assert_eq!(unmark(h, &elements(h), 3), None, "a heading's hashes are text to delete");
    }

    #[test]
    fn vertical_moves_by_displayed_row_toward_the_goal_x_and_stops_at_the_edges() {
        let text = "a".repeat(25);
        let rows = [0..10, 11..20, 21..25];
        let x_of = |b: usize| (b - rows.iter().rev().find(|r| r.start <= b).unwrap().start) as f32 * 8.0;
        assert_eq!(vertical(&text, &rows, x_of, 3, Dir::Down, 24.0), Some(14));
        assert_eq!(vertical(&text, &rows, x_of, 14, Dir::Down, 72.0), Some(25), "a short row: its end");
        assert_eq!(vertical(&text, &rows, x_of, 3, Dir::Up, 24.0), None);
        assert_eq!(vertical(&text, &rows, x_of, 22, Dir::Down, 8.0), None);
        assert_eq!(row_edges(&rows, 14), (11, 20));
    }

    #[test]
    fn a_megabyte_document_s_active_paragraph_reveals_in_time() {
        let para = "Some **bold** and _italic_ text with a [link](https://example.com) and `code`, then more words to make a long line that wraps.\n";
        let doc: String = std::iter::repeat_n(format!("{para}\n"), 1_000_000 / (para.len() + 1)).collect();
        assert!(doc.len() > 900_000);
        let block = format!("{para}{para}{para}");
        let started = Instant::now();
        let nodes = elements(&block);
        let m = markers(&block, &nodes);
        let d = Display::new(&block, &nodes, &[30]);
        let took = started.elapsed();
        assert!(!m.is_empty() && d.text.len() < block.len());
        assert!(took < Duration::from_millis(1), "the reveal took {took:?}");
    }
}
