//! Links in terminal output: anything from `http://` or `https://` to the next space or quote,
//! trailing punctuation trimmed. The hovered one is underlined and Cmd+click opens it in the
//! default browser, as Terminal.app does. A URL wider than a row runs on across rows: the
//! terminal wrapped it (the row's wrap flag), or Claude Code cut it at its wrap, the pane's
//! width in a message and two cells short of it in the input box, and went on under its indent.
//! Pure: the element decides what is hovered.

/// One link on a row, as cell columns (`end` exclusive) and the text to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

fn starts_scheme(chars: &[char], i: usize) -> bool {
    const HTTPS: [char; 8] = ['h', 't', 't', 'p', 's', ':', '/', '/'];
    const HTTP: [char; 7] = ['h', 't', 't', 'p', ':', '/', '/'];
    chars[i..].starts_with(&HTTPS) || chars[i..].starts_with(&HTTP)
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

/// A link's piece on one row (`end` exclusive), with the whole URL: a link across rows is one
/// span per row, each carrying the same URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub url: String,
}

/// Rows a URL may run across, above and below the rows asked about.
pub const LINK_CONTEXT: usize = 8;

/// Every link in `rows`, as its pieces on each row. Row `i` runs on into row `i + 1` when the
/// terminal wrapped it (`wrapped[i]`) or when Claude Code cut a URL there (`cut`): the URL ends
/// the row's text exactly at the program's wrap, the pane's width in a message and two cells
/// short of it in the input box, and the rest sits under the next row's indent. A group without
/// "http" in it is skipped before the scan.
pub fn links_in_rows(rows: &[Vec<char>], wrapped: &[bool]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut first = 0;
    while first < rows.len() {
        // The group's rows as (row, first cell taken, cells taken), and their text joined: a
        // wrap takes whole rows; a cut drops the row's trailing blanks and the next row's
        // indent, so the URL reads whole.
        let mut pieces = vec![(first, 0, rows[first].len())];
        let mut joined: Vec<char> = rows[first].clone();
        let mut i = first;
        while let Some(next) = rows.get(i + 1) {
            if wrapped.get(i).copied().unwrap_or(false) {
                pieces.push((i + 1, 0, next.len()));
                joined.extend_from_slice(next);
            } else if let Some((end, indent)) = cut(&joined, pieces.last().copied().expect("a group has a row"), &rows[i], next, cut_width(rows, i)) {
                let piece = pieces.last_mut().expect("a group has a row");
                let blanks = (piece.1 + piece.2).saturating_sub(end);
                piece.2 -= blanks;
                joined.truncate(joined.len() - blanks);
                pieces.push((i + 1, indent, next.len() - indent));
                joined.extend_from_slice(&next[indent..]);
            } else {
                break;
            }
            i += 1;
        }
        if joined.iter().collect::<String>().contains("http") {
            // Where each piece starts in the joined text.
            let starts: Vec<usize> = pieces.iter().scan(0, |at, p| { let s = *at; *at += p.2; Some(s) }).collect();
            for link in links_in_row(&joined) {
                for (k, &(row, skip, take)) in pieces.iter().enumerate() {
                    let (piece_start, piece_end) = (starts[k], starts[k] + take);
                    let (s, e) = (link.start.max(piece_start), link.end.min(piece_end));
                    if s < e {
                        out.push(Span { row, start: s - piece_start + skip, end: e - piece_start + skip, url: link.url.clone() });
                    }
                }
            }
        }
        first = i + 1;
    }
    out
}

/// Where Claude Code cuts a token wider than its wrap on row `i`: the pane's width, or two cells
/// short of it in the input box, whose rows hang under a `\u{276f}` prompt with a two-cell indent.
fn cut_width(rows: &[Vec<char>], i: usize) -> usize {
    rows[i].len() - if in_input_box(rows, i) { crate::reflow::INPUT_BOX_INSET } else { 0 }
}

/// Whether row `i` is in Claude Code's input box: it, or a run of two-cell-indented rows above
/// it, hangs under a row that opens with the prompt `\u{276f}`.
pub fn in_input_box(rows: &[Vec<char>], i: usize) -> bool {
    for row in rows[..=i].iter().rev().take(64) {
        if row.first() == Some(&'\u{276f}') {
            return true;
        }
        if !(row.len() > 2 && row[0] == ' ' && row[1] == ' ' && row[2] != ' ') {
            return false;
        }
    }
    false
}

/// Whether a URL that ends `row`'s text was cut there and goes on in `next`: (the cell after the
/// URL's piece, the indent `next`'s piece starts at). `joined` is the group's text so far, `row`
/// its last row and `piece` that row's (first cell taken, cells taken), so a URL already across
/// rows is seen whole. Claude Code cuts a token only when it is wider than its wrap, so the
/// piece ends exactly at `cut_width` and the pieces together are wider than it; a row that
/// wrapped on a space never reaches the wrap, its space does. The rest sits under the row's
/// indent, never deeper than the URL began.
fn cut(joined: &[char], (_, skip, _): (usize, usize, usize), row: &[char], next: &[char], cut_width: usize) -> Option<(usize, usize)> {
    let end = row.iter().rposition(|c| !c.is_whitespace())? + 1;
    if end != cut_width || end <= skip {
        return None;
    }
    let prefix = &joined[..joined.len() - (row.len() - end)];
    let link = links_in_row(prefix).pop().filter(|l| l.end == prefix.len())?;
    let piece_start = prefix.len() - (end - skip);
    let col = link.start.saturating_sub(piece_start) + skip;
    let indent = next.iter().take_while(|c| **c == ' ').count();
    let rest = next[indent..].iter().take_while(|c| !ends_link(**c)).count();
    (rest > 0 && indent <= col && (link.end - link.start) + rest > cut_width).then_some((end, indent))
}

/// The span covering cell (`row`, `col`), if any.
pub fn span_at(spans: &[Span], row: usize, col: usize) -> Option<&Span> {
    spans.iter().find(|s| s.row == row && s.start <= col && col < s.end)
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

    fn spans(rows: &[&str], wrapped: &[bool]) -> Vec<(usize, usize, usize, String)> {
        let rows: Vec<Vec<char>> = rows.iter().map(|r| row(r)).collect();
        links_in_rows(&rows, wrapped).into_iter().map(|s| (s.row, s.start, s.end, s.url)).collect()
    }

    #[test]
    fn a_url_the_terminal_wrapped_runs_on_across_the_rows() {
        let url = "https://csarko.sh/a/very/long/path".to_string();
        assert_eq!(spans(&["see https://csarko.sh/a/very/lo", "ng/path now"], &[true, false]), [(0, 4, 31, url.clone()), (1, 0, 7, url)]);
        let three = "https://csarko.sh/abcdefghijklmnopqrstuvwxyz".to_string();
        assert_eq!(spans(&["https://csarko.sh/ab", "cdefghijklmnopqrstuv", "wxyz"], &[true, true, false]), [(0, 0, 20, three.clone()), (1, 0, 20, three.clone()), (2, 0, 4, three)]);
    }

    #[test]
    fn a_url_a_program_cut_at_the_pane_s_width_runs_on_without_the_flag() {
        // A program that wraps at the width writes each row itself: no wrap flag, the same cut.
        let url = "https://csarko.sh/a/very/long/path".to_string();
        assert_eq!(spans(&["see https://csarko.sh/a/very/lo", "ng/path now"], &[false, false]), [(0, 4, 31, url.clone()), (1, 0, 7, url)]);
        // A scheme the cut split is still found.
        assert_eq!(spans(&["words words words ht", "tps://x.io/p and"], &[true, false]), [(0, 18, 20, "https://x.io/p".to_string()), (1, 0, 12, "https://x.io/p".to_string())]);
    }

    #[test]
    fn a_url_cut_across_three_rows_of_the_input_box_is_whole() {
        let url = "https://github.com/csarkosh/ide-arkos/actions/runs/39877328412123456789023".to_string();
        let rows = ["\u{276f}\u{a0}YZ0123456789abcdef                    ", "  https://github.com/csarkosh/ide-arko  ", "  s/actions/runs/398773284121234567890  ", "  23 now                                "];
        assert_eq!(spans(&rows, &[false; 4]), [(1, 2, 38, url.clone()), (2, 2, 38, url.clone()), (3, 2, 4, url)]);
        assert!(in_input_box(&rows.iter().map(|r| row(r)).collect::<Vec<_>>(), 3));
        assert!(!in_input_box(&[row("\u{25cf} a bullet"), row("  its continuation")], 1));
    }

    #[test]
    fn a_full_row_without_a_url_at_its_end_joins_nothing() {
        // A frame's right border fills the last column on every row: the rows stay apart, and
        // a URL ends at the border.
        let rows = ["\u{2502} https://a.io/x        \u{2502}", "\u{2502} and https://b.io  \u{2502}"];
        assert_eq!(spans(&rows, &[false, false]).iter().map(|s| s.3.as_str()).collect::<Vec<_>>(), ["https://a.io/x", "https://b.io"]);
    }

    #[test]
    fn a_url_claude_code_cut_at_its_own_width_runs_on_under_the_indent() {
        // Its input box wraps two cells short of a 40-column pane; the rest sits under the prompt.
        let url = "https://github.com/csarkosh/ide-arkos/actions/runs/39877328412".to_string();
        let rows = ["\u{276f}\u{a0}YZ0123456789abcdef                    ", "  https://github.com/csarkosh/ide-arko  ", "  s/actions/runs/39877328412            "];
        assert_eq!(spans(&rows, &[false; 3]), [(1, 2, 38, url.clone()), (2, 2, 28, url)]);
        // In a message, under a bullet's indent, with prose after the URL.
        let guide = "https://github.com/csarkosh/ide-arkos/blob/main/docs/guide.md#keys".to_string();
        let rows = ["\u{25cf} See https://github.com/csarkosh/ide-arkos/blob/main/docs/g", "  uide.md#keys for the keys."];
        assert_eq!(spans(&rows, &[false; 2]), [(0, 6, 60, guide.clone()), (1, 2, 14, guide)]);
    }

    #[test]
    fn a_row_that_ends_on_a_url_before_a_wrapped_word_stays_two_things() {
        assert_eq!(spans(&["\u{25cf} see https://a.io/c", "  and then more"], &[false; 2]), [(0, 6, 20, "https://a.io/c".to_string())]);
        // A message row that wrapped on a space ends short of the pane, its space taking the
        // last cell: not a cut, whatever follows.
        let rows = ["\u{25cf} https://github.com/csarkosh/ide-arkos/blob ", "  configuration and more"];
        assert_eq!(spans(&rows, &[false; 2]), [(0, 2, 44, "https://github.com/csarkosh/ide-arkos/blob".to_string())]);
        // Deeper than the URL began is a new line of its own.
        assert_eq!(spans(&["see https://csarko.sh/a/very/lo", "      ng/path"], &[false; 2]), [(0, 4, 31, "https://csarko.sh/a/very/lo".to_string())]);
    }

    #[test]
    fn a_url_that_ends_before_the_last_column_stops_there() {
        // Rows are the pane's width: this URL ends well short of it.
        assert_eq!(spans(&["see https://csarko.sh/x                 ", "ng/path                                 "], &[false, false]), [(0, 4, 23, "https://csarko.sh/x".to_string())]);
    }

    #[test]
    fn a_wrapped_row_whose_url_ended_at_a_space_does_not_take_the_next_row() {
        assert_eq!(spans(&["https://a.io/x then some more words!", "https://b.io"], &[true, false]), [(0, 0, 14, "https://a.io/x".to_string()), (1, 0, 12, "https://b.io".to_string())]);
        assert!(spans(&["plain", "rows"], &[true, false]).is_empty());
    }

    #[test]
    fn the_span_under_a_cell_is_found_on_either_row() {
        let rows: Vec<Vec<char>> = ["see https://csarko.sh/a/very/lo", "ng/path now"].iter().map(|r| row(r)).collect();
        let spans = links_in_rows(&rows, &[true, false]);
        assert_eq!(span_at(&spans, 0, 3), None);
        assert_eq!(span_at(&spans, 0, 30).map(|s| (s.row, s.start, s.end)), Some((0, 4, 31)));
        assert_eq!(span_at(&spans, 1, 6).map(|s| (s.row, s.start, s.end)), Some((1, 0, 7)));
        assert_eq!(span_at(&spans, 1, 7), None);
    }
}
