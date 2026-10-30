//! A code block's parts for its chrome (spec `2026-10-30-code-block-design.md`): its fences and
//! code lines, the language list and the fence edit that sets one, the copy text, and a block's
//! colour runs split by line. Pure, over the document's lines.

use crate::md_blocks::{Block, BlockKind};
use std::ops::Range;

/// A fenced block's opening fence: its line, its indent, its marker (three or more backticks or
/// tildes) and its info string; and the closing fence's line, when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub open: usize,
    pub close: Option<usize>,
    pub indent: String,
    pub marker: String,
    pub info: String,
}

/// The opening fence of a code block, when it is fenced.
pub fn fence(lines: &[String], block: &Block) -> Option<Fence> {
    if !matches!(block.kind, BlockKind::CodeBlock { .. }) {
        return None;
    }
    let open = block.lines.start;
    let line = lines.get(open)?;
    let body = line.trim_start_matches(' ');
    let indent = &line[..line.len() - body.len()];
    let ch = body.chars().next().filter(|&c| c == '`' || c == '~')?;
    let n = body.chars().take_while(|&c| c == ch).count();
    if n < 3 || indent.len() > 3 {
        return None;
    }
    let marker = body[..n].to_string();
    let info = body[n..].trim().to_string();
    let end = block.lines.end.min(lines.len());
    let close = (open + 1..end).find(|&i| {
        let t = lines[i].trim();
        t.len() >= n && t.chars().all(|c| c == ch)
    });
    Some(Fence { open, close, indent: indent.to_string(), marker, info })
}

/// The lines a code block's code is on: between its fences (to its last line of text when it is
/// not closed), or its text's lines for an indented block. Empty for a fenced block with no line
/// between its fences, and for anything else.
pub fn code_lines(lines: &[String], block: &Block) -> Range<usize> {
    if !matches!(block.kind, BlockKind::CodeBlock { .. }) {
        return 0..0;
    }
    let end = block.lines.end.min(lines.len());
    let last_text = (block.lines.start..end).rev().find(|&i| !lines[i].trim().is_empty()).map_or(block.lines.start, |i| i + 1);
    match fence(lines, block) {
        Some(f) => f.open + 1..f.close.unwrap_or(last_text).max(f.open + 1),
        None => block.lines.start..last_text,
    }
}

/// The languages the pill offers: the info-string id each writes and its name.
pub const LANGUAGES: [(&str, &str); 33] = [
    ("", "Plain text"),
    ("bash", "Bash"),
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("diff", "Diff"),
    ("dockerfile", "Dockerfile"),
    ("go", "Go"),
    ("graphql", "GraphQL"),
    ("html", "HTML"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("json", "JSON"),
    ("kotlin", "Kotlin"),
    ("lua", "Lua"),
    ("makefile", "Makefile"),
    ("markdown", "Markdown"),
    ("mermaid", "Mermaid"),
    ("objc", "Objective-C"),
    ("perl", "Perl"),
    ("php", "PHP"),
    ("python", "Python"),
    ("r", "R"),
    ("ruby", "Ruby"),
    ("rust", "Rust"),
    ("scala", "Scala"),
    ("sql", "SQL"),
    ("swift", "Swift"),
    ("toml", "TOML"),
    ("typescript", "TypeScript"),
    ("xml", "XML"),
    ("yaml", "YAML"),
];

/// Other ids that name a listed language.
const ALIASES: [(&str, &str); 12] = [("sh", "bash"), ("shell", "bash"), ("zsh", "bash"), ("c++", "cpp"), ("cs", "csharp"), ("js", "javascript"), ("ts", "typescript"), ("py", "python"), ("rb", "ruby"), ("rs", "rust"), ("yml", "yaml"), ("md", "markdown")];

/// The pill's name for an info string: its first word's language, or that word itself.
pub fn language_name(info: &str) -> String {
    let id = info.split_whitespace().next().unwrap_or("").to_lowercase();
    let id = ALIASES.iter().find(|(a, _)| *a == id).map_or(id.as_str(), |(_, to)| to).to_string();
    LANGUAGES.iter().find(|(i, _)| *i == id).map_or(id.clone(), |(_, name)| name.to_string())
}

/// The languages whose name or id holds `q`, case aside, in the list's order.
pub fn filter_languages(q: &str) -> Vec<(&'static str, &'static str)> {
    let q = q.to_lowercase();
    LANGUAGES.iter().copied().filter(|(id, name)| q.is_empty() || name.to_lowercase().contains(&q) || id.contains(&q) || ALIASES.iter().any(|(a, to)| to == id && *a == q)).collect()
}

/// The opening fence line with its language set to `id` (its indent and marker kept, and anything
/// after the first word of the old info string), as the line's number and its new text.
pub fn set_language(lines: &[String], block: &Block, id: &str) -> Option<(usize, String)> {
    let f = fence(lines, block)?;
    let rest = f.info.split_once(char::is_whitespace).map(|(_, r)| r.trim()).filter(|r| !r.is_empty());
    let info = match (id.is_empty(), rest) {
        (true, _) => String::new(),
        (false, Some(r)) => format!("{id} {r}"),
        (false, None) => id.to_string(),
    };
    Some((f.open, format!("{}{}{info}", f.indent, f.marker)))
}

/// The code Copy puts on the clipboard: the code lines, an indented block's indent taken off.
pub fn copy_text(lines: &[String], block: &Block) -> String {
    let fenced = fence(lines, block).is_some();
    lines[code_lines(lines, block)]
        .iter()
        .map(|l| if fenced { l.as_str() } else { l.strip_prefix("    ").or_else(|| l.strip_prefix('\t')).unwrap_or(l) })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs over lines joined with one-byte line breaks (each `(length, value)`), split into each line's
/// runs (`lens` the lines' lengths), the breaks left out.
pub fn split_by_lines<T: Clone>(runs: &[(usize, T)], lens: &[usize]) -> Vec<Vec<(usize, T)>> {
    let mut out: Vec<Vec<(usize, T)>> = lens.iter().map(|_| Vec::new()).collect();
    let (mut line, mut left) = (0usize, lens.first().copied().unwrap_or(0));
    let mut skip_break = false;
    for (len, value) in runs {
        let mut len = *len;
        while len > 0 && line < lens.len() {
            if skip_break {
                skip_break = false;
                len -= 1;
                continue;
            }
            let take = len.min(left);
            if take > 0 {
                out[line].push((take, value.clone()));
            }
            len -= take;
            left -= take;
            if left == 0 {
                line += 1;
                left = lens.get(line).copied().unwrap_or(0);
                skip_break = true;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md_blocks;

    fn doc(s: &str) -> (Vec<String>, Vec<Block>) {
        let lines: Vec<String> = s.split('\n').map(String::from).collect();
        let blocks = md_blocks::split(&lines);
        (lines, blocks)
    }

    #[test]
    fn a_code_block_s_fences_and_code_lines() {
        let (l, b) = doc("```rust title\nfn a() {}\nfn b() {}\n```\n\nafter");
        let f = fence(&l, &b[0]).unwrap();
        assert_eq!((f.open, f.close, f.marker.as_str(), f.info.as_str()), (0, Some(3), "```", "rust title"));
        assert_eq!(code_lines(&l, &b[0]), 1..3);
        let (l, b) = doc("~~~~\nx\n~~~~");
        assert_eq!((fence(&l, &b[0]).unwrap().marker.as_str(), code_lines(&l, &b[0])), ("~~~~", 1..2));
        let (l, b) = doc("```\nopen\nstill");
        assert_eq!((fence(&l, &b[0]).unwrap().close, code_lines(&l, &b[0])), (None, 1..3), "unclosed: to its last line");
        let (l, b) = doc("    indented\n    code\n\nafter");
        assert_eq!((fence(&l, &b[0]), code_lines(&l, &b[0])), (None, 0..2));
        let (l, b) = doc("```\n```");
        assert!(code_lines(&l, &b[0]).is_empty(), "no line between the fences");
        let (l, b) = doc("para");
        assert!(code_lines(&l, &b[0]).is_empty());
    }

    #[test]
    fn languages_are_named_filtered_and_set_on_the_fence() {
        assert_eq!(language_name("rust"), "Rust");
        assert_eq!(language_name("py"), "Python", "an alias");
        assert_eq!(language_name(""), "Plain text");
        assert_eq!(language_name("elixir"), "elixir", "not listed: the id itself");
        assert_eq!(filter_languages("py").iter().map(|(_, n)| *n).collect::<Vec<_>>(), vec!["Python"]);
        assert_eq!(filter_languages("script").iter().map(|(_, n)| *n).collect::<Vec<_>>(), vec!["JavaScript", "TypeScript"]);
        assert_eq!(filter_languages("").len(), LANGUAGES.len());
        let (l, b) = doc("  ```js title=\"x\"\n  a\n  ```");
        assert_eq!(set_language(&l, &b[0], "python"), Some((0, "  ```python title=\"x\"".to_string())), "the indent, marker and the rest kept");
        let (l, b) = doc("```rust\nx\n```");
        assert_eq!(set_language(&l, &b[0], ""), Some((0, "```".to_string())), "Plain text");
    }

    #[test]
    fn copy_takes_the_code_without_fences_or_an_indent() {
        let (l, b) = doc("```\na\n  b\n```");
        assert_eq!(copy_text(&l, &b[0]), "a\n  b");
        let (l, b) = doc("    x\n    y");
        assert_eq!(copy_text(&l, &b[0]), "x\ny");
    }

    #[test]
    fn runs_split_by_line_and_the_breaks_drop_out() {
        // "ab\ncde": runs of 2 'x', 1 break, 3 'y'; and one run across the break.
        assert_eq!(split_by_lines(&[(2, 'x'), (1, 'n'), (3, 'y')], &[2, 3]), vec![vec![(2, 'x')], vec![(3, 'y')]]);
        assert_eq!(split_by_lines(&[(6, 'z')], &[2, 3]), vec![vec![(2, 'z')], vec![(3, 'z')]]);
        assert_eq!(split_by_lines(&[(1, 'a'), (1, 'n'), (2, 'b')], &[1, 0, 1]), vec![vec![(1, 'a')], vec![], vec![(1, 'b')]], "an empty line");
    }
}
