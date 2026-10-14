//! Links in terminal output: anything from `http://` or `https://` to the next space or quote,
//! trailing punctuation trimmed. The hovered one is underlined and Cmd+click opens it in the
//! default browser, as Terminal.app does. Pure: the element decides what is hovered.

/// One link on a row, as cell columns (`end` exclusive) and the text to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

fn starts_scheme(chars: &[char], i: usize) -> bool {
    let rest: String = chars[i..chars.len().min(i + 8)].iter().collect();
    rest.starts_with("https://") || rest.starts_with("http://")
}

fn ends_link(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`')
}

/// Every link on a row of cells. A URL runs from its scheme to the next space or quote;
/// trailing `.,;:!?` and a `)` or `]` with no opening match inside the URL are not part of it.
pub fn links_in_row(chars: &[char]) -> Vec<Link> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !starts_scheme(chars, i) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = start;
        while end < chars.len() && !ends_link(chars[end]) {
            end += 1;
        }
        loop {
            let Some(&last) = chars[start..end].last() else { break };
            let trim = match last {
                '.' | ',' | ';' | ':' | '!' | '?' => true,
                ')' => chars[start..end].iter().filter(|c| **c == '(').count() < chars[start..end].iter().filter(|c| **c == ')').count(),
                ']' => chars[start..end].iter().filter(|c| **c == '[').count() < chars[start..end].iter().filter(|c| **c == ']').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            end -= 1;
        }
        let url: String = chars[start..end].iter().collect();
        // More than the scheme itself.
        if url.len() > url.find("//").map(|k| k + 2).unwrap_or(0) {
            out.push(Link { start, end, url });
        }
        i = end.max(start + 1);
    }
    out
}

/// The link covering cell column `col`, if any.
pub fn link_at(links: &[Link], col: usize) -> Option<&Link> {
    links.iter().find(|l| l.start <= col && col < l.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn urls(s: &str) -> Vec<(usize, usize, String)> {
        links_in_row(&row(s)).into_iter().map(|l| (l.start, l.end, l.url)).collect()
    }

    #[test]
    fn links_start_at_the_scheme_and_stop_at_space_or_quote() {
        assert_eq!(urls("see https://csarko.sh now"), [(4, 21, "https://csarko.sh".to_string())]);
        assert_eq!(urls("http://a.b/c?d=1&e=2 and https://x.y"), [(0, 20, "http://a.b/c?d=1&e=2".to_string()), (25, 36, "https://x.y".to_string())]);
        assert_eq!(urls("\"https://q.io/p\" 'https://r.io'"), [(1, 15, "https://q.io/p".to_string()), (18, 30, "https://r.io".to_string())]);
        assert_eq!(urls("<https://t.io/x>"), [(1, 15, "https://t.io/x".to_string())]);
        assert!(urls("httpx://nope and ftp://no").is_empty());
        assert!(urls("https://").is_empty(), "a bare scheme is not a link");
    }

    #[test]
    fn trailing_punctuation_and_unmatched_brackets_are_left_out() {
        assert_eq!(urls("go to https://csarko.sh."), [(6, 23, "https://csarko.sh".to_string())]);
        assert_eq!(urls("(https://csarko.sh/research)"), [(1, 27, "https://csarko.sh/research".to_string())]);
        assert_eq!(urls("https://en.wikipedia.org/wiki/Rust_(language)"), [(0, 45, "https://en.wikipedia.org/wiki/Rust_(language)".to_string())], "a balanced paren stays");
        assert_eq!(urls("https://a.io/x), then"), [(0, 14, "https://a.io/x".to_string())]);
        assert_eq!(urls("https://a.io/?q=1;"), [(0, 17, "https://a.io/?q=1".to_string())]);
    }

    #[test]
    fn the_link_under_a_column_is_found() {
        let links = links_in_row(&row("see https://csarko.sh now"));
        assert_eq!(link_at(&links, 3), None);
        assert_eq!(link_at(&links, 4).map(|l| l.url.as_str()), Some("https://csarko.sh"));
        assert_eq!(link_at(&links, 20).map(|l| l.url.as_str()), Some("https://csarko.sh"));
        assert_eq!(link_at(&links, 21), None);
    }
}
