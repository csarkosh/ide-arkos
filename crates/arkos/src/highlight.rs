//! Syntax colours for the viewer (spec 6): syntect's TextMate grammars, with a theme built from
//! the config theme's nine viewer colours so every file matches the window. The syntax set
//! loads once, off the UI thread, at launch.

use crate::palette::Rgb;
use crate::theme::ViewerColors;
use crate::viewer::Span;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::SyntaxSet;

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

    fn highlight_with(&self, syntax: Option<&syntect::parsing::SyntaxReference>, lines: &[String]) -> Vec<Vec<Span>> {
        let Some(syntax) = syntax else {
            return lines.iter().map(|l| plain(l, self.text)).collect();
        };
        let mut h = syntect::easy::HighlightLines::new(syntax, &self.theme);
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            // The grammars expect the newline; the spans are measured on the line without it.
            let with_nl = format!("{line}\n");
            let mut spans = Vec::new();
            let mut pos = 0;
            match h.highlight_line(&with_nl, &self.syntaxes) {
                Ok(regions) => {
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
                }
                Err(_) => spans = plain(line, self.text),
            }
            out.push(spans);
        }
        out
    }
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
}
