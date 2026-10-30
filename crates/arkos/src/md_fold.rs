//! A markdown document's structure (spec `2026-10-30-structure-design.md`): its headings and their
//! sections, which blocks a set of folds hides, the outline tree, the heading in view, a section's
//! preview text, and moving a block as one edit. Pure, over the blocks and lines.

use crate::md_blocks::{self, Block, BlockKind, Inline};
use std::collections::BTreeSet;
use std::ops::Range;

/// A fold's key: the heading's text and its place among the headings with that text.
pub type FoldKey = (String, usize);

/// A heading: its block, its level, its text as it reads, and its place among the headings with
/// the same text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub block: usize,
    pub level: u8,
    pub text: String,
    pub nth: usize,
}

impl Head {
    pub fn key(&self) -> FoldKey {
        (self.text.clone(), self.nth)
    }
}

/// The document's headings in order.
pub fn headings(blocks: &[Block], lines: &[String]) -> Vec<Head> {
    let mut out: Vec<Head> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let BlockKind::Heading(level) = b.kind else { continue };
        let end = b.lines.end.min(lines.len());
        let source = lines[b.lines.start.min(end)..end].join("\n");
        let text = match md_blocks::inline(&source, &b.kind).into_iter().next() {
            Some(Inline::Heading(_, runs)) => runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string(),
            _ => source.trim().trim_start_matches('#').trim().to_string(),
        };
        let nth = out.iter().filter(|h| h.text == text).count();
        out.push(Head { block: i, level, text, nth });
    }
    out
}

/// The blocks heading `i`'s section holds: after it, up to the next heading of its level or higher.
pub fn section(heads: &[Head], i: usize, block_count: usize) -> Range<usize> {
    let h = &heads[i];
    let end = heads[i + 1..].iter().find(|n| n.level <= h.level).map_or(block_count, |n| n.block);
    h.block + 1..end
}

/// For each block, whether a folded heading above it hides it.
pub fn hidden(heads: &[Head], folded: &BTreeSet<FoldKey>, block_count: usize) -> Vec<bool> {
    let mut out = vec![false; block_count];
    for (i, h) in heads.iter().enumerate() {
        if folded.contains(&h.key()) {
            for b in section(heads, i, block_count) {
                out[b] = true;
            }
        }
    }
    out
}

/// The folded headings whose sections hold `block`: unfolding them shows it.
pub fn hiding(heads: &[Head], folded: &BTreeSet<FoldKey>, block: usize, block_count: usize) -> Vec<FoldKey> {
    heads.iter().enumerate().filter(|(i, h)| folded.contains(&h.key()) && section(heads, *i, block_count).contains(&block)).map(|(_, h)| h.key()).collect()
}

/// One row of the outline: its heading (an index into the headings), how deep it sits under the
/// headings above it, and whether headings sit under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineRow {
    pub head: usize,
    pub depth: usize,
    pub has_children: bool,
}

/// The outline: each heading under the nearest heading above it of a lower level.
pub fn outline(heads: &[Head]) -> Vec<OutlineRow> {
    let mut stack: Vec<u8> = Vec::new();
    let mut rows: Vec<OutlineRow> = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        while stack.last().is_some_and(|&l| l >= h.level) {
            stack.pop();
        }
        rows.push(OutlineRow { head: i, depth: stack.len(), has_children: false });
        stack.push(h.level);
    }
    for i in 0..rows.len() {
        rows[i].has_children = rows.get(i + 1).is_some_and(|n| n.depth > rows[i].depth);
    }
    rows
}

/// The outline rows shown when the rows in `collapsed` (by heading index) hide their children.
pub fn visible_rows(rows: &[OutlineRow], collapsed: &BTreeSet<usize>) -> Vec<OutlineRow> {
    let mut out = Vec::new();
    let mut hide_below: Option<usize> = None;
    for r in rows {
        if let Some(d) = hide_below {
            if r.depth > d {
                continue;
            }
            hide_below = None;
        }
        out.push(*r);
        if collapsed.contains(&r.head) {
            hide_below = Some(r.depth);
        }
    }
    out
}

/// The heading whose section the view's top block is in: the last heading at or above it.
pub fn current(heads: &[Head], top_block: usize) -> Option<usize> {
    heads.iter().rposition(|h| h.block <= top_block)
}

/// A section's preview: the heading, then up to `max` lines of the section's text, each trimmed,
/// blank lines and fences left out.
pub fn preview(heads: &[Head], i: usize, blocks: &[Block], lines: &[String], max: usize) -> Vec<String> {
    let mut out = vec![heads[i].text.clone()];
    let blocks_in = section(heads, i, blocks.len());
    let from = blocks.get(blocks_in.start).map_or(lines.len(), |b| b.lines.start);
    let to = blocks.get(blocks_in.end).map_or(lines.len(), |b| b.lines.start);
    out.extend(lines[from.min(lines.len())..to.min(lines.len())].iter().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with("```") && !l.starts_with("~~~")).take(max).map(str::to_string));
    out
}

/// Moving block `from` to before block `to` (`blocks.len()` for the end) as one edit: lines
/// `lines` of the document become `text`, and the moved block starts at line `at` afterwards. A
/// blank line stays between the block and its neighbours. `None` when the block would not move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveEdit {
    pub lines: Range<usize>,
    pub text: Vec<String>,
    pub at: usize,
}

pub fn move_block(lines: &[String], blocks: &[Block], from: usize, to: usize) -> Option<MoveEdit> {
    if from >= blocks.len() || to > blocks.len() || to == from || to == from + 1 {
        return None;
    }
    let src = blocks[from].lines.clone();
    let mut content: Vec<String> = lines[src.clone()].to_vec();
    while content.last().is_some_and(|l| l.trim().is_empty()) {
        content.pop();
    }
    if content.is_empty() {
        return None;
    }
    let target = blocks.get(to).map_or(lines.len(), |b| b.lines.start);
    let mut rest: Vec<String> = lines[..src.start].to_vec();
    rest.extend_from_slice(&lines[src.end..]);
    let mut at = if target > src.start { target - src.len() } else { target };
    let mut moved = content;
    // A blank line after it, and before it when the line above is text.
    moved.push(String::new());
    if at > 0 && rest.get(at - 1).is_some_and(|l| !l.trim().is_empty()) {
        moved.insert(0, String::new());
        at += 1;
    }
    if at.saturating_sub(1) >= rest.len() && rest.last().is_some_and(|l| l.trim().is_empty()) && moved.first().is_some_and(|l| l.is_empty()) {
        // At the very end after a blank line: no second blank line.
        moved.remove(0);
        at -= 1;
    }
    let insert_at = if moved.first().is_some_and(|l| l.is_empty()) && at > 0 { at - 1 } else { at };
    let mut new = rest.clone();
    new.splice(insert_at..insert_at, moved);
    // The changed region, as one edit of the old lines.
    let lo = src.start.min(target);
    let hi = src.end.max(target).min(lines.len());
    let grow = new.len() as isize - lines.len() as isize;
    let text = new[lo..(hi as isize + grow).max(lo as isize) as usize].to_vec();
    Some(MoveEdit { lines: lo..hi, text, at })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> (Vec<String>, Vec<Block>) {
        let lines: Vec<String> = s.split('\n').map(String::from).collect();
        let blocks = md_blocks::split(&lines);
        (lines, blocks)
    }

    fn apply(lines: &[String], e: &MoveEdit) -> Vec<String> {
        let mut out = lines[..e.lines.start].to_vec();
        out.extend(e.text.iter().cloned());
        out.extend_from_slice(&lines[e.lines.end..]);
        out
    }

    const DOC: &str = "# One\n\na\n\n## Two\n\nb\n\n## Two\n\nc\n\n# Three\n\nd";

    #[test]
    fn headings_carry_their_text_level_and_place_among_namesakes() {
        let (lines, blocks) = doc("# **Bold** title\n\n## Two\n\n## Two");
        let h = headings(&blocks, &lines);
        assert_eq!(h.iter().map(|h| (h.text.as_str(), h.level, h.nth)).collect::<Vec<_>>(), vec![("Bold title", 1, 0), ("Two", 2, 0), ("Two", 2, 1)]);
    }

    #[test]
    fn a_fold_hides_its_section_up_to_the_next_heading_of_its_level_or_higher() {
        let (lines, blocks) = doc(DOC);
        let h = headings(&blocks, &lines);
        assert_eq!(section(&h, 0, blocks.len()), 1..6, "One holds a, Two, b, Two, c");
        assert_eq!(section(&h, 1, blocks.len()), 3..4);
        let folded: BTreeSet<FoldKey> = [("Two".to_string(), 1)].into();
        let hide = hidden(&h, &folded, blocks.len());
        assert_eq!(hide.iter().enumerate().filter(|(_, &x)| x).map(|(i, _)| i).collect::<Vec<_>>(), vec![5], "only the second Two's c");
        assert_eq!(hiding(&h, &folded, 5, blocks.len()), vec![("Two".to_string(), 1)]);
        let (lines2, blocks2) = doc(&format!("intro\n\n{DOC}"));
        let h2 = headings(&blocks2, &lines2);
        assert!(hidden(&h2, &folded, blocks2.len())[6], "a paragraph added above: the fold still names the same heading");
    }

    #[test]
    fn the_outline_nests_headings_and_collapses_rows() {
        let (lines, blocks) = doc("# A\n\n## B\n\n### C\n\n## D\n\n# E");
        let h = headings(&blocks, &lines);
        let rows = outline(&h);
        assert_eq!(rows.iter().map(|r| (r.depth, r.has_children)).collect::<Vec<_>>(), vec![(0, true), (1, true), (2, false), (1, false), (0, false)]);
        let shown = visible_rows(&rows, &[1].into());
        assert_eq!(shown.iter().map(|r| r.head).collect::<Vec<_>>(), vec![0, 1, 3, 4], "B collapsed hides C");
        assert_eq!(current(&h, 0), Some(0));
        assert_eq!(current(&h, 2), Some(2));
        assert_eq!(current(&h, 3), Some(3), "a heading is its own section's first block");
        let (l2, b2) = doc("intro\n\n# A");
        assert_eq!(current(&headings(&b2, &l2), 0), None, "above the first heading");
    }

    #[test]
    fn a_preview_is_the_heading_and_the_section_s_first_lines() {
        let (lines, blocks) = doc("# A\n\nfirst line\nsecond\n\n```\ncode\n```\n\n# B\n\nother");
        let h = headings(&blocks, &lines);
        assert_eq!(preview(&h, 0, &blocks, &lines, 3), vec!["A", "first line", "second", "code"]);
    }

    #[test]
    fn a_block_moves_with_blank_lines_kept_around_it() {
        let (lines, blocks) = doc("# H\n\none\n\ntwo\n\nthree");
        let e = move_block(&lines, &blocks, 3, 1).expect("three moves up");
        assert_eq!(apply(&lines, &e), ["# H", "", "three", "", "one", "", "two", ""].map(String::from).to_vec());
        assert_eq!(e.at, 2);
        let e = move_block(&lines, &blocks, 1, 4).expect("one moves to the end");
        assert_eq!(apply(&lines, &e), ["# H", "", "two", "", "three", "", "one", ""].map(String::from).to_vec());
        assert_eq!(e.at, 6);
        let e = move_block(&lines, &blocks, 0, 3).expect("the heading moves down");
        assert_eq!(apply(&lines, &e), ["one", "", "two", "", "# H", "", "three"].map(String::from).to_vec());
        assert_eq!(move_block(&lines, &blocks, 2, 2), None, "onto itself");
        assert_eq!(move_block(&lines, &blocks, 2, 3), None, "just after itself");
    }
}
