//! Bold, italic, strikethrough and code as edits of one line of markdown (spec
//! `2026-10-30-typing-rules-design.md`, section 6): wrap the selection, the word under the caret
//! or an empty pair at it; or take an element's delimiters away when the selection is the
//! element, or the caret is in it. Pure, in bytes of the line.

use crate::md_blocks::{self, Node, NodeKind};
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bold,
    Italic,
    Strike,
    Code,
}

impl Format {
    fn is(self, kind: &NodeKind) -> bool {
        matches!((self, kind), (Format::Bold, NodeKind::Strong) | (Format::Italic, NodeKind::Emphasis) | (Format::Strike, NodeKind::Strike) | (Format::Code, NodeKind::Code))
    }

    /// The delimiter written for this format; italic is `*` where `_` would touch a word (an
    /// underscore inside a word is not emphasis).
    fn delim(self, inside_word: bool) -> &'static str {
        match self {
            Format::Bold => "**",
            Format::Italic if inside_word => "*",
            Format::Italic => "_",
            Format::Strike => "~~",
            Format::Code => "`",
        }
    }
}

/// One edit of a line: the byte range replaced, its new text, and the selection after it, in
/// bytes of the new line (empty for a caret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    pub range: Range<usize>,
    pub text: String,
    pub select: Range<usize>,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}' | '-')
}

/// Every element of `fmt` in the tree, with the range of the text between its delimiters.
fn elements_of(nodes: &[Node], fmt: Format, out: &mut Vec<(Range<usize>, Range<usize>)>) {
    for n in nodes {
        if fmt.is(&n.kind) {
            let inner = match (n.children.first(), n.children.last()) {
                // A code span's text keeps the spaces inside its backticks.
                (Some(_), Some(_)) if n.kind == NodeKind::Code => n.children[0].span.clone(),
                (Some(a), Some(b)) => a.span.start..b.span.end,
                _ => n.span.start..n.span.start,
            };
            out.push((n.span.clone(), inner));
        }
        elements_of(&n.children, fmt, out);
    }
}

/// The edit Cmd+B (Bold), Cmd+I (Italic) or the menu's Strikethrough and Code make on `line` with
/// the selection `sel` (bytes; empty for a caret). `None` when there is nothing to do (a selection
/// of only spaces).
pub fn toggle(line: &str, sel: Range<usize>, fmt: Format) -> Option<LineEdit> {
    let sel = sel.start.min(line.len())..sel.end.min(line.len()).max(sel.start.min(line.len()));
    let mut found = Vec::new();
    elements_of(&md_blocks::elements(line), fmt, &mut found);
    let unwrap = |span: &Range<usize>, inner: &Range<usize>, at: Range<usize>| {
        let open = inner.start - span.start;
        let shift = |o: usize| o.clamp(inner.start, inner.end) - open;
        LineEdit { range: span.clone(), text: line[inner.clone()].to_string(), select: shift(at.start)..shift(at.end) }
    };
    let before = |at: usize| line[..at].chars().next_back();
    let after = |at: usize| line[at..].chars().next();
    if sel.start < sel.end {
        if let Some((span, inner)) = found.iter().find(|(span, inner)| *inner == sel || *span == sel) {
            return Some(unwrap(span, inner, inner.clone()));
        }
        // Spaces at the ends stay outside: `** word**` is not bold.
        let text = &line[sel.clone()];
        let lead = text.len() - text.trim_start().len();
        let trail = text.len() - text.trim_end().len();
        if lead == text.len() {
            return None;
        }
        let (a, b) = (sel.start + lead, sel.end - trail);
        let inside = before(a).is_some_and(word_char) || after(b).is_some_and(word_char);
        let d = fmt.delim(inside);
        return Some(LineEdit { range: a..b, text: format!("{d}{}{d}", &line[a..b]), select: a + d.len()..b + d.len() });
    }
    let c = sel.start;
    // The innermost element the caret is in, ends included.
    if let Some((span, inner)) = found.iter().filter(|(span, _)| span.start <= c && c <= span.end).min_by_key(|(span, _)| span.len()) {
        return Some(unwrap(span, inner, c..c));
    }
    let (mut a, mut b) = (c, c);
    while let Some(ch) = before(a).filter(|&ch| word_char(ch)) {
        a -= ch.len_utf8();
    }
    while let Some(ch) = after(b).filter(|&ch| word_char(ch)) {
        b += ch.len_utf8();
    }
    if a < b {
        let d = fmt.delim(before(a).is_some_and(word_char) || after(b).is_some_and(word_char));
        return Some(LineEdit { range: a..b, text: format!("{d}{}{d}", &line[a..b]), select: c + d.len()..c + d.len() });
    }
    let d = fmt.delim(false);
    Some(LineEdit { range: c..c, text: format!("{d}{d}"), select: c + d.len()..c + d.len() })
}

/// What a link made from the clipboard points at (spec section 7): one line that is a web or mail
/// address, or a path to something that exists (absolute, `~`, or relative to `base_dir`, the
/// document's folder), written relative to `base_dir` when it is inside it. In `<...>` when it
/// holds a space or a parenthesis. `None` for anything else.
pub fn link_target(clip: &str, base_dir: &Path, home: Option<&Path>) -> Option<String> {
    let t = clip.trim();
    if t.is_empty() || t.contains('\n') {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"].iter().any(|p| lower.starts_with(p)) {
        return (!t.contains(char::is_whitespace)).then(|| t.to_string());
    }
    let full: PathBuf = match t.strip_prefix("~/") {
        Some(rest) => home?.join(rest),
        None if Path::new(t).is_absolute() => PathBuf::from(t),
        None => base_dir.join(t),
    };
    if !full.exists() {
        return None;
    }
    let written = match full.strip_prefix(base_dir) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ if !Path::new(t).is_absolute() && !t.starts_with("~/") => t.to_string(),
        _ => full.to_string_lossy().into_owned(),
    };
    Some(if written.contains([' ', '(', ')']) { format!("<{written}>") } else { written })
}

/// The link Cmd+V makes over a selection on one line (`selected`) from the clipboard, when
/// `link_target` finds one in it.
pub fn paste_link(selected: &str, clip: &str, base_dir: &Path, home: Option<&Path>) -> Option<String> {
    if selected.trim().is_empty() || selected.contains('\n') {
        return None;
    }
    link_target(clip, base_dir, home).map(|target| format!("[{selected}]({target})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(line: &str, sel: Range<usize>, fmt: Format) -> (String, Range<usize>) {
        let e = toggle(line, sel, fmt).expect("an edit");
        let mut out = line.to_string();
        out.replace_range(e.range.clone(), &e.text);
        (out, e.select)
    }

    #[test]
    fn a_selection_is_wrapped_and_stays_selected_with_its_spaces_outside() {
        assert_eq!(apply("make this bold", 5..9, Format::Bold), ("make **this** bold".into(), 7..11));
        assert_eq!(apply("make this bold", 4..10, Format::Bold), ("make **this** bold".into(), 7..11), "the spaces stay outside");
        assert!(toggle("a b", 1..2, Format::Bold).is_none());
        assert_eq!(apply("x y", 0..1, Format::Strike).0, "~~x~~ y");
        assert_eq!(apply("x y", 0..1, Format::Code).0, "`x` y");
    }

    #[test]
    fn italic_is_an_underscore_between_words_and_a_star_inside_one() {
        assert_eq!(apply("one two", 4..7, Format::Italic).0, "one _two_");
        assert_eq!(apply("unbelievable", 2..6, Format::Italic).0, "un*beli*evable", "an underscore inside a word is not emphasis");
    }

    #[test]
    fn the_element_selected_or_under_the_caret_loses_its_delimiters() {
        assert_eq!(apply("make **this** bold", 7..11, Format::Bold), ("make this bold".into(), 5..9), "its text selected");
        assert_eq!(apply("make **this** bold", 5..13, Format::Bold), ("make this bold".into(), 5..9), "the element selected");
        assert_eq!(apply("make **this** bold", 9..9, Format::Bold), ("make this bold".into(), 7..7), "the caret in it keeps its place in the text");
        assert_eq!(apply("an _italic_ word", 6..6, Format::Italic).0, "an italic word");
        assert_eq!(apply("a `code` b", 4..4, Format::Code).0, "a code b");
        assert_eq!(apply("**bold _both_**", 9..9, Format::Italic).0, "**bold both**", "the italic inside the bold");
    }

    #[test]
    fn with_no_selection_the_word_is_wrapped_or_an_empty_pair_goes_in() {
        assert_eq!(apply("make this bold", 7..7, Format::Bold), ("make **this** bold".into(), 9..9), "the caret keeps its place in the word");
        assert_eq!(apply("make this bold", 9..9, Format::Bold).0, "make **this** bold", "at the word's end");
        assert_eq!(apply("a  b", 2..2, Format::Bold), ("a **** b".into(), 4..4), "between words: an empty pair, the caret inside");
        assert_eq!(apply("", 0..0, Format::Italic), ("__".into(), 1..1));
        assert_eq!(apply("café au lait", 2..2, Format::Bold).0, "**café** au lait");
    }

    #[test]
    fn an_address_or_an_existing_path_pasted_over_a_selection_makes_a_link() {
        let dir = std::env::temp_dir().join(format!("arkos-paste-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("notes dir")).unwrap();
        std::fs::write(dir.join("notes.md"), "x").unwrap();
        let home = Some(dir.as_path());
        assert_eq!(paste_link("Arkos", "https://csarko.sh/arkos\n", &dir, home).as_deref(), Some("[Arkos](https://csarko.sh/arkos)"));
        assert_eq!(paste_link("mail", "mailto:a@b.c", &dir, home).as_deref(), Some("[mail](mailto:a@b.c)"));
        assert_eq!(paste_link("the notes", "notes.md", &dir, home).as_deref(), Some("[the notes](notes.md)"), "relative to the document");
        assert_eq!(paste_link("abs", &dir.join("notes.md").to_string_lossy(), &dir, home).as_deref(), Some("[abs](notes.md)"), "inside the folder: written relative");
        assert_eq!(paste_link("d", "notes dir", &dir, home).as_deref(), Some("[d](<notes dir>)"), "a space: in angle brackets");
        assert_eq!(paste_link("h", "~/notes.md", &dir, home).as_deref(), Some("[h](notes.md)"));
        assert_eq!(paste_link("x", "missing.md", &dir, home), None, "a path that does not exist pastes as text");
        assert_eq!(paste_link("x", "two\nlines", &dir, home), None);
        assert_eq!(paste_link("x", "just words", &dir, home), None);
        assert_eq!(paste_link("", "https://a.b", &dir, home), None, "no selection");
        assert_eq!(paste_link("a\nb", "https://a.b", &dir, home), None, "a selection over two lines");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
