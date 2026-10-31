//! Syntax colours for the viewer (spec 6): syntect's TextMate grammars, with a theme built from
//! the config theme's nine viewer colours so every file matches the window. The syntax set
//! loads once, off the UI thread, at launch.

use crate::palette::Rgb;
use crate::theme::ViewerColors;
use crate::viewer::Span;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use syntect::highlighting::{Color, FontStyle, HighlightIterator, HighlightState, ScopeSelectors, Style, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

fn color(c: Rgb) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: 255 }
}

fn item(selector: &str, c: Rgb) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(selector).expect("a valid scope selector"),
        style: StyleModifier { foreground: Some(color(c)), background: None, font_style: None },
    }
}

/// A syntect theme from the nine viewer colours (spec 7: "viewer: syntax colours mapped onto
/// the highlighter's scopes"). syntect picks the item whose selector matches most specifically,
/// not the first or last listed, so `keyword.operator` beats `keyword` and
/// `punctuation.definition.comment` beats `punctuation`. The bundled grammars scope declaration keywords
/// (`fn`, `let`, `struct`, `def`, `class`, `function`) as `storage.type`, so it is a keyword.
pub fn scope_theme(colors: &ViewerColors, text: Rgb) -> Theme {
    Theme {
        name: Some("arkos".into()),
        author: None,
        settings: ThemeSettings { foreground: Some(color(text)), ..Default::default() },
        scopes: vec![
            item("keyword, storage.modifier, storage.type", colors.keyword),
            item("string, punctuation.definition.string", colors.string),
            item("comment, punctuation.definition.comment", colors.comment),
            item("constant", colors.constant),
            item("constant.numeric", colors.number),
            item("entity.name.function, support.function, variable.function", colors.function),
            item("entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.impl, support.type, support.class", colors.type_name),
            item("keyword.operator", colors.operator),
            item("punctuation", colors.punctuation),
        ],
    }
}

pub struct Highlighter {
    /// Shared with the highlighters `with_theme` makes, so a theme change never reloads it.
    syntaxes: Arc<SyntaxSet>,
    theme: Theme,
    text: Rgb,
}

impl Highlighter {
    /// Loads the default grammars (about 100 ms): call it on the background executor.
    pub fn new(colors: &ViewerColors, text: Rgb) -> Self {
        Highlighter { syntaxes: Arc::new(SyntaxSet::load_defaults_newlines()), theme: scope_theme(colors, text), text }
    }

    /// The same grammars under another theme's colours; cheap enough for the UI thread.
    pub fn with_theme(&self, colors: &ViewerColors, text: Rgb) -> Self {
        Highlighter { syntaxes: self.syntaxes.clone(), theme: scope_theme(colors, text), text }
    }

    /// One span list per line, contiguous and covering the line; an empty line has none. A file
    /// no grammar claims gets one plain span per line. Reads nothing from disk: the grammar comes
    /// from the file name, then its extension, then the first line already loaded.
    pub fn highlight(&self, path: &Path, lines: &[String]) -> Vec<Vec<Span>> {
        self.highlight_with(self.syntax_for(path, lines), lines)
    }

    /// The grammar `highlight` colours a file with: from the file name, then its extension, then
    /// the first line; `None` when none claims it.
    pub fn syntax_for(&self, path: &Path, lines: &[String]) -> Option<&syntect::parsing::SyntaxReference> {
        let by_ext = |s: Option<&std::ffi::OsStr>| s.and_then(|s| s.to_str()).and_then(|s| self.syntaxes.find_syntax_by_extension(s));
        by_ext(path.file_name())
            .or_else(|| by_ext(path.extension()))
            .or_else(|| lines.first().and_then(|l| self.syntaxes.find_syntax_by_first_line(l)))
    }

    /// `highlight` for a fenced code block: the grammar comes from the info string's first word
    /// ("rust", "py", "sh"), and an unknown or empty one gives plain spans.
    pub fn highlight_token(&self, info: &str, lines: &[String]) -> Vec<Vec<Span>> {
        let token = info.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next().unwrap_or("");
        let syntax = (!token.is_empty()).then(|| self.syntaxes.find_syntax_by_token(token)).flatten();
        self.highlight_with(syntax, lines)
    }

    /// `highlight` with the grammar already picked (`syntax_for`), so a caller that also needs
    /// the grammar picks it once.
    pub fn highlight_with(&self, syntax: Option<&syntect::parsing::SyntaxReference>, lines: &[String]) -> Vec<Vec<Span>> {
        CodeHighlight::new(self, syntax, lines).1
    }
}

/// The grammar's state at the end of a line: what the next line's colours start from.
type LineState = (ParseState, HighlightState);

/// How many lines one `update` or `finish` pass recolours before it leaves the rest for the next
/// pass (`pending`), so a keystroke that changes the colours of a long file's tail never holds the
/// frame.
pub const REHIGHLIGHT_BUDGET: usize = 2000;

/// A code document's colours kept while it is edited: the grammar, and the grammar's state at
/// the end of every line, so an edit recolours from its first changed line only until a line
/// ends in the state it ended in before (the next line, for most edits). Built with the spans on
/// the background thread (`new`); each edit splices it on the UI thread (`update`).
pub struct CodeHighlight {
    syntaxes: Arc<SyntaxSet>,
    theme: Theme,
    text: Rgb,
    /// The grammar's index in `syntaxes`; `None` for a file no grammar claims, whose spans are
    /// plain and whose states are never read.
    syntax: Option<usize>,
    /// The state at the end of each line, one per line of the document; `None` for a line
    /// inserted by an edit and not coloured yet, which no state can equal.
    states: Vec<Option<LineState>>,
    /// The first line whose spans and state are stale after a pass stopped at its budget.
    pub pending: Option<usize>,
}

impl CodeHighlight {
    /// Colours every line of `lines` under `syntax` and remembers the state at the end of each,
    /// returning the highlight and the spans. Without a grammar every line is one plain span.
    pub fn new(h: &Highlighter, syntax: Option<&syntect::parsing::SyntaxReference>, lines: &[String]) -> (CodeHighlight, Vec<Vec<Span>>) {
        let syntax = syntax.and_then(|s| h.syntaxes.syntaxes().iter().position(|c| std::ptr::eq(c, s)));
        let mut code = CodeHighlight { syntaxes: h.syntaxes.clone(), theme: h.theme.clone(), text: h.text, syntax, states: vec![None; lines.len()], pending: None };
        let mut spans = vec![Vec::new(); lines.len()];
        code.run(lines, &mut spans, 0, usize::MAX);
        (code, spans)
    }

    /// Whether a grammar other than Plain Text colours the file.
    pub fn coloured(&self) -> bool {
        self.syntax.is_some_and(|ix| self.syntaxes.syntaxes()[ix].name != "Plain Text")
    }

    /// An edit replaced `removed` lines at `prefix` with `inserted` new ones: `lines` is the text
    /// now and `spans` its spans before, spliced here to match. The lines before `prefix` keep
    /// their colours; from `prefix` on they are made again until a line ends in the state it had
    /// before the edit, at most `budget` lines in this pass (the rest is `pending`, the stale
    /// colours shown meanwhile).
    pub fn update(&mut self, lines: &[String], spans: &mut Vec<Vec<Span>>, prefix: usize, removed: usize, inserted: usize, budget: usize) {
        let prefix = prefix.min(self.states.len()).min(spans.len());
        let removed = removed.min(self.states.len() - prefix).min(spans.len() - prefix);
        self.states.splice(prefix..prefix + removed, std::iter::repeat_n(None, inserted));
        spans.splice(prefix..prefix + removed, std::iter::repeat_with(Vec::new).take(inserted));
        debug_assert_eq!(self.states.len(), lines.len());
        // A pass still pending starts no later than before: the states after it are stale, so an
        // edit below it cannot stop at one of them.
        let from = self.pending.take().map_or(prefix, |p| p.min(prefix));
        self.run(lines, spans, from, budget);
    }

    /// Goes on from `pending`; true when something was still to do.
    pub fn finish(&mut self, lines: &[String], spans: &mut Vec<Vec<Span>>, budget: usize) -> bool {
        match self.pending.take() {
            Some(from) => {
                self.run(lines, spans, from, budget);
                true
            }
            None => false,
        }
    }

    /// Colours from line `from`, stopping after a line (past `from`) whose new end state equals
    /// its old one, at the budget (leaving `pending`), or at the end.
    fn run(&mut self, lines: &[String], spans: &mut [Vec<Span>], from: usize, budget: usize) {
        let n = lines.len().min(self.states.len()).min(spans.len());
        let Some(ix) = self.syntax else {
            // Plain: the stale lines are those the splice left empty; a pass without a grammar
            // never converges on a state, so it recolours every line that is not coloured yet.
            for i in from..n {
                if spans[i].is_empty() {
                    spans[i] = plain(&lines[i], self.text);
                }
            }
            return;
        };
        let syntax = &self.syntaxes.syntaxes()[ix];
        let highlighter = syntect::highlighting::Highlighter::new(&self.theme);
        let (mut parse, mut high) = match from.checked_sub(1).and_then(|i| self.states[i].clone()) {
            Some(s) => s,
            None => (ParseState::new(syntax), HighlightState::new(&highlighter, ScopeStack::new())),
        };
        let mut done = 0;
        for i in from..n {
            if done >= budget {
                self.pending = Some(i);
                return;
            }
            done += 1;
            // The grammars expect the newline; the spans are measured on the line without it.
            let with_nl = format!("{}\n", lines[i]);
            spans[i] = match parse.parse_line(&with_nl, &self.syntaxes) {
                Ok(ops) => {
                    let regions = HighlightIterator::new(&mut high, &ops, &with_nl, &highlighter);
                    spans_of(regions)
                }
                Err(_) => plain(&lines[i], self.text),
            };
            let state = (parse.clone(), high.clone());
            let same = i > from && self.states[i].as_ref() == Some(&state);
            self.states[i] = Some(state);
            if same {
                return;
            }
        }
    }
}

/// The spans of one highlighted line from syntect's regions.
fn spans_of<'a>(regions: impl Iterator<Item = (Style, &'a str)>) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut pos = 0;
    for (style, text) in regions {
        let text = text.strip_suffix('\n').unwrap_or(text);
        if text.is_empty() {
            continue;
        }
        let c = style.foreground;
        spans.push(Span {
            start: pos,
            end: pos + text.len(),
            color: Rgb { r: c.r, g: c.g, b: c.b },
            bold: style.font_style.contains(FontStyle::BOLD),
            italic: style.font_style.contains(FontStyle::ITALIC),
        });
        pos += text.len();
    }
    spans
}

fn plain(line: &str, text: Rgb) -> Vec<Span> {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![Span { start: 0, end: line.len(), color: text, bold: false, italic: false }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    fn colors() -> ViewerColors {
        ViewerColors {
            keyword: rgb(0xc9a4e8),
            string: rgb(0x8fd9a8),
            comment: rgb(0x7c8798),
            number: rgb(0xe8c97a),
            function: rgb(0x8ab8f0),
            type_name: rgb(0x7fd4d4),
            constant: rgb(0xf0a8a8),
            operator: rgb(0xc8ccd4),
            punctuation: rgb(0xa8b0bc),
        }
    }

    #[test]
    fn rust_keywords_strings_and_comments_get_their_colours() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let lines: Vec<String> = ["fn main() {", "    let s = \"hi\"; // note", "}"].iter().map(|s| s.to_string()).collect();
        let spans = h.highlight(Path::new("/p/main.rs"), &lines);
        assert_eq!(spans.len(), 3);
        let at = |line: usize, byte: usize| spans[line].iter().find(|s| s.start <= byte && byte < s.end).map(|s| s.color).expect("covered");
        assert_eq!(at(0, 0), colors().keyword, "fn");
        assert_eq!(at(1, 13), colors().string, "the string literal");
        assert_eq!(at(1, 20), colors().comment, "the comment");
        assert_eq!(at(0, 3), colors().function, "main");
        assert_eq!(at(1, 4), colors().keyword, "let");
        assert_eq!(at(1, 12), colors().string, "the opening quote");
        assert_eq!(at(1, 18), colors().comment, "the comment marker");
        // Every byte of every line is covered by exactly one span, in order.
        for (i, line) in lines.iter().enumerate() {
            let mut pos = 0;
            for s in &spans[i] {
                assert_eq!(s.start, pos, "line {i}: spans are contiguous");
                pos = s.end;
            }
            assert_eq!(pos, line.len(), "line {i}: spans cover the line");
        }
    }

    #[test]
    fn with_theme_recolours_with_the_new_colours() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let other = ViewerColors { keyword: rgb(0x123456), ..colors() };
        let spans = h.with_theme(&other, rgb(0xd6dae0)).highlight(Path::new("/p/main.rs"), &["fn main() {}".to_string()]);
        assert_eq!(spans[0][0].color, rgb(0x123456), "fn takes the new theme's keyword colour");
    }

    #[test]
    fn the_grammar_comes_from_the_name_then_the_extension_then_the_first_line() {
        let h = Highlighter::new(&colors(), rgb(0x010203));
        let name = |path: &str, first: &str| h.syntax_for(Path::new(path), &[first.to_string()]).map(|s| s.name.clone());
        assert_eq!(name("/p/Makefile", "all:").as_deref(), Some("Makefile"));
        assert_eq!(name("/p/main.rs", "fn main() {}").as_deref(), Some("Rust"));
        assert_eq!(name("/p/notes.txt", "hello").as_deref(), Some("Plain Text"));
        assert_eq!(name("/p/run", "#!/bin/bash").as_deref(), Some("Bourne Again Shell (bash)"));
        assert_eq!(name("/p/notes.unknownext", "some text"), None);
    }

    #[test]
    fn an_unknown_extension_is_one_plain_span_per_line() {
        let h = Highlighter::new(&colors(), rgb(0x010203));
        let lines: Vec<String> = ["some text", ""].iter().map(|s| s.to_string()).collect();
        let spans = h.highlight(Path::new("/p/notes.unknownext"), &lines);
        assert_eq!(spans[0], vec![Span { start: 0, end: 9, color: rgb(0x010203), bold: false, italic: false }]);
        assert_eq!(spans[1], Vec::<Span>::new(), "an empty line has no span");
    }

    #[test]
    fn the_theme_maps_the_nine_colours_onto_scopes() {
        let t = scope_theme(&colors(), rgb(0x090909));
        assert_eq!(t.settings.foreground, Some(Color { r: 9, g: 9, b: 9, a: 255 }));
        let selectors: Vec<String> = t.scopes.iter().map(|i| i.scope.selectors.iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>().join("|")).collect();
        assert_eq!(t.scopes.len(), 9);
        assert!(selectors[0].contains("keyword"), "{selectors:?}");
    }

    fn lines(src: &str) -> Vec<String> {
        src.split('\n').map(str::to_string).collect()
    }

    /// `update` after replacing `removed` lines at `prefix` with `with`, checked against a fresh
    /// full highlight of the new text; returns the new lines and whether a pass was left pending.
    fn edited(code: &mut CodeHighlight, spans: &mut Vec<Vec<Span>>, old: &[String], prefix: usize, removed: usize, with: &[&str], budget: usize) -> Vec<String> {
        let mut new: Vec<String> = old[..prefix].to_vec();
        new.extend(with.iter().map(|s| s.to_string()));
        new.extend_from_slice(&old[prefix + removed..]);
        code.update(&new, spans, prefix, removed, with.len(), budget);
        new
    }

    const SRC: &str = "fn main() {\n    let s = \"hi\"; // note\n    let n = 42;\n    println!(\"{s} {n}\");\n}\n// tail\nfn other() -> u32 {\n    7\n}";

    #[test]
    fn an_incremental_update_matches_a_full_highlight_for_an_insert_a_delete_and_a_paste() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let path = Path::new("/p/main.rs");
        let old = lines(SRC);
        let (mut code, mut spans) = CodeHighlight::new(&h, h.syntax_for(path, &old), &old);
        assert_eq!(spans, h.highlight(path, &old), "new is the full highlight");
        // Typing in one line.
        let new = edited(&mut code, &mut spans, &old, 2, 1, &["    let n = 420;"], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new), "an edit inside a line");
        assert_eq!(code.pending, None);
        // Deleting two lines.
        let new = edited(&mut code, &mut spans, &new, 1, 2, &[], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new), "a delete of two lines");
        // Pasting three.
        let new = edited(&mut code, &mut spans, &new, 1, 0, &["    // a", "    let x = 'c';", "    x;"], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new), "a paste of three lines");
        assert_eq!(spans.len(), new.len());
    }

    #[test]
    fn an_opened_block_comment_recolours_to_the_end_and_closing_it_restores_the_rest() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let path = Path::new("/p/main.rs");
        let old = lines(SRC);
        let (mut code, mut spans) = CodeHighlight::new(&h, h.syntax_for(path, &old), &old);
        let new = edited(&mut code, &mut spans, &old, 0, 1, &["/* fn main() {"], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new));
        assert!(spans.last().unwrap().iter().all(|s| s.color == colors().comment), "the last line is now a comment");
        let new = edited(&mut code, &mut spans, &new, 0, 1, &["fn main() {"], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new), "closed again, the tail takes its old colours back");
    }

    #[test]
    fn the_budget_leaves_a_pending_tail_that_finish_completes() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let path = Path::new("/p/main.rs");
        let old = lines(SRC);
        let (mut code, mut spans) = CodeHighlight::new(&h, h.syntax_for(path, &old), &old);
        let stale = spans.clone();
        let new = edited(&mut code, &mut spans, &old, 0, 1, &["/* fn main() {"], 3);
        assert_eq!(code.pending, Some(3), "three lines done, the fourth is where the next pass starts");
        assert_eq!(&spans[3..], &stale[3..], "the lines not reached keep their old colours");
        assert!(code.finish(&new, &mut spans, 3));
        assert_eq!(code.pending, Some(6));
        assert!(code.finish(&new, &mut spans, usize::MAX));
        assert_eq!(code.pending, None);
        assert_eq!(spans, h.highlight(path, &new));
        assert!(!code.finish(&new, &mut spans, usize::MAX), "nothing pending: nothing to do");
    }

    #[test]
    fn an_edit_above_a_pending_pass_starts_from_the_edit_and_one_below_it_from_the_pass() {
        let h = Highlighter::new(&colors(), rgb(0xd6dae0));
        let path = Path::new("/p/main.rs");
        let old = lines(SRC);
        let (mut code, mut spans) = CodeHighlight::new(&h, h.syntax_for(path, &old), &old);
        let new = edited(&mut code, &mut spans, &old, 0, 1, &["/* fn main() {"], 2);
        assert_eq!(code.pending, Some(2));
        // An edit below the pending line: the pass picks up at the pending line, not the edit.
        let new = edited(&mut code, &mut spans, &new, 7, 1, &["    8"], usize::MAX);
        assert_eq!(code.pending, None);
        assert_eq!(spans, h.highlight(path, &new));
        let new = edited(&mut code, &mut spans, &new, 0, 1, &["/* x"], 2);
        let new = edited(&mut code, &mut spans, &new, 0, 1, &["fn main() {"], usize::MAX);
        assert_eq!(spans, h.highlight(path, &new), "an edit above the pending line starts there");
    }

    #[test]
    fn a_file_no_grammar_claims_is_plain_spans_and_updates_stay_plain() {
        let h = Highlighter::new(&colors(), rgb(0x010203));
        let old = lines("some\ntext");
        let (mut code, mut spans) = CodeHighlight::new(&h, None, &old);
        assert!(!code.coloured());
        assert_eq!(spans, vec![plain("some", rgb(0x010203)), plain("text", rgb(0x010203))]);
        let new = edited(&mut code, &mut spans, &old, 1, 1, &["more", ""], usize::MAX);
        assert_eq!(spans, vec![plain("some", rgb(0x010203)), plain("more", rgb(0x010203)), Vec::new()]);
        assert_eq!(new.len(), 3);
        let rs = lines("fn x() {}");
        assert!(CodeHighlight::new(&h, h.syntax_for(Path::new("/p/a.rs"), &rs), &rs).0.coloured());
        assert!(!CodeHighlight::new(&h, h.syntax_for(Path::new("/p/a.txt"), &rs), &rs).0.coloured(), "Plain Text is not coloured");
    }
}
