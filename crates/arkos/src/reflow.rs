//! Copying from a terminal that a program wrapped for its own width. Claude Code wraps its prose
//! and writes each visual line with a real newline, so the selection's text has a break at every
//! row even where the program only ran out of room; alacritty's own join covers only the rows the
//! terminal wrapped. `reflow` tells the program's wraps from the breaks it meant and joins the
//! wraps. Pure, so it is tested.
use unicode_width::UnicodeWidthStr;

/// Joins the lines a program wrapped for the terminal's width and keeps the ones it broke on
/// purpose. `columns` is the terminal's width, `first_col` the column the text starts at on
/// its first line (a selection that starts mid-row), both in cells. Widths are display widths
/// (`unicode-width`), so CJK and emoji count as the cells they take.
pub fn reflow(text: &str, columns: usize, first_col: usize) -> String {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    // Each line's width without its trailing blanks; the first starts `first_col` cells in.
    let widths: Vec<usize> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let w = l.trim_end().width();
            if i == 0 { w.saturating_add(first_col) } else { w }
        })
        .collect();
    let wrap = widths.iter().copied().max().unwrap_or(0).min(columns);
    // Text that never came near the terminal's width was not wrapped for it: every line ends
    // where its program ended it.
    if wrap < columns.saturating_sub(12) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(lines[0]);
    for i in 1..lines.len() {
        let (a, b) = (lines[i - 1], lines[i]);
        let b_t = b.trim_start_matches(' ');
        match join(a.trim_end(), widths[i - 1], b, b_t, columns, wrap) {
            Some(sep) => {
                // The output ends with `a` as the input has it, trailing blanks and all.
                out.truncate(out.len() - (a.len() - a.trim_end().len()));
                out.push_str(sep);
                out.push_str(b_t);
            }
            None => {
                out.push('\n');
                out.push_str(b);
            }
        }
    }
    if body.len() < text.len() {
        out.push('\n');
    }
    out
}

/// What goes between `a` and `b` when `b` continues the line `a` was wrapped from (a space, or
/// nothing inside a URL or a path broken mid-word), or None when `a` ends where its program ended
/// it. `a` is trimmed on the right and `width` cells wide, `b_t` is `b` without its indent.
fn join(a: &str, width: usize, b: &str, b_t: &str, columns: usize, wrap: usize) -> Option<&'static str> {
    if a.is_empty() || b_t.is_empty() {
        return None;
    }
    if starts_structure(b_t) || starts_box(a.trim_start()) || starts_box(b_t) {
        return None;
    }
    // Wider than the terminal: alacritty already joined the rows the terminal wrapped, so this
    // newline came from the program (a log line, a long `ls` row).
    if width > columns {
        return None;
    }
    let next = b_t.split(' ').next().unwrap_or("").width();
    if width.saturating_add(1).saturating_add(next) <= wrap {
        return None;
    }
    let last = a.rsplit(' ').next().unwrap_or("");
    let mid_word = width == wrap && !b.starts_with(' ') && last.contains('/');
    Some(if mid_word { "" } else { " " })
}

/// Whether a line opens a list item, a quote, a heading or a code fence, which never continue the
/// line before them.
fn starts_structure(line: &str) -> bool {
    const MARKERS: [&str; 8] = ["- ", "* ", "+ ", "\u{2022} ", "\u{25cf} ", "\u{25cb} ", "\u{25d0} ", "\u{23fa} "];
    if MARKERS.iter().any(|m| line.starts_with(m)) {
        return true;
    }
    if line.starts_with(['>', '#']) || line.starts_with("```") || line.starts_with("~~~") {
        return true;
    }
    if line.starts_with("[ ]") || line.starts_with("[x]") {
        return true;
    }
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.len() < line.len() && (rest.starts_with(". ") || rest.starts_with(") "))
}

/// Whether a line starts with a box-drawing or block character: a table, or a frame of Claude
/// Code's.
fn starts_box(line: &str) -> bool {
    line.chars().next().is_some_and(|c| ('\u{2500}'..='\u{259f}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(ls: &[&str]) -> String {
        ls.join("\n")
    }

    fn cells(s: &str) -> usize {
        s.width()
    }

    // Claude Code at 80 columns: a message's first paragraph after its bullet, continuation
    // lines indented two spaces (as in the multitasker shoot).
    const BULLET: [&str; 3] = [
        "● I've started all three agents in parallel: the limiter and its tests, the",
        "  server wiring with route tests, and the README. Once they've all reported",
        "  back, I'll review the changes, run npm test, and fix anything that fails.",
    ];

    // A later paragraph of the same message: every line indented two spaces, the widest at 78.
    const PARAGRAPH: [&str; 3] = [
        "  The limiter is done: src/limiter.js plus 6 tests, all passing. One gap it",
        "  flagged: with limit: 0, Retry-After would come out as NaN. Nothing uses that",
        "  setting, so I'm leaving it. Now waiting on the server agent.",
    ];

    const README: [&str; 2] = [
        "  The README is done. It now has a short \"Rate limit\" subsection under the API",
        "  table and a note on the headers.",
    ];

    // A "Decision for you:" paragraph whose first line fills all 80 columns.
    const DECISION: [&str; 3] = [
        "  Decision for you: the limiter keeps its counts in memory, so a restart forgets",
        "  them and two server processes count separately. That is fine for one box; for",
        "  more, say so and I'll move the counts to Redis.",
    ];

    #[test]
    fn the_fixtures_have_the_widths_claude_code_gave_them() {
        assert_eq!(BULLET.map(cells), [75, 75, 75]);
        assert_eq!(PARAGRAPH.map(cells), [75, 78, 62]);
        assert_eq!(README.map(cells), [78, 34]);
        assert_eq!(DECISION.map(cells), [80, 79, 49]);
    }

    #[test]
    fn a_wrapped_paragraph_of_three_lines_is_one_line() {
        let want = "  The limiter is done: src/limiter.js plus 6 tests, all passing. One gap it flagged: with limit: 0, \
                    Retry-After would come out as NaN. Nothing uses that setting, so I'm leaving it. Now waiting on the server agent.";
        assert_eq!(reflow(&lines(&PARAGRAPH), 80, 0), want);
    }

    #[test]
    fn a_two_line_paragraph_is_one_line() {
        let want = "  The README is done. It now has a short \"Rate limit\" subsection under the API table and a note on the headers.";
        assert_eq!(reflow(&lines(&README), 80, 0), want);
    }

    #[test]
    fn a_bullet_s_continuation_lines_join_it_without_their_indent() {
        let want = "● I've started all three agents in parallel: the limiter and its tests, the server wiring with route tests, \
                    and the README. Once they've all reported back, I'll review the changes, run npm test, and fix anything that fails.";
        assert_eq!(reflow(&lines(&BULLET), 80, 0), want);
    }

    #[test]
    fn two_bullets_in_a_row_never_join() {
        // The second item's line fills the width, so only the list marker keeps the third apart.
        let text = lines(&[
            "  - Write limiter.js and its tests: a sliding window per key, pruned on every",
            "    hit call.",
            "  - Wire the limiter into the server routes, with a 429 and a Retry-After header",
            "  - Document the rate limit in the README, under the API table.",
        ]);
        let want = lines(&[
            "  - Write limiter.js and its tests: a sliding window per key, pruned on every hit call.",
            "  - Wire the limiter into the server routes, with a 429 and a Retry-After header",
            "  - Document the rate limit in the README, under the API table.",
        ]);
        assert_eq!(reflow(&text, 80, 0), want);
        // A numbered item after one that fills the selection's width stays on its own too.
        let numbered = lines(&[
            "  1. Keep the counts in memory: nothing new to run, and a restart forgets them.",
            "  2. Move them to Redis, which every server process shares.",
        ]);
        assert_eq!(cells("  1. Keep the counts in memory: nothing new to run, and a restart forgets them."), 79);
        assert_eq!(reflow(&numbered, 80, 0), numbered);
        let marked = lines(&["  1. Keep the counts in memory: nothing new to run, and a restart forgets them.", "  2) Move them."]);
        assert_eq!(reflow(&marked, 80, 0), marked);
    }

    #[test]
    fn the_comparison_is_between_physical_lines_so_a_short_last_line_ends_the_paragraph() {
        let mut text = PARAGRAPH.to_vec();
        text.push("  Done.");
        let want = lines(&[
            "  The limiter is done: src/limiter.js plus 6 tests, all passing. One gap it flagged: with limit: 0, \
             Retry-After would come out as NaN. Nothing uses that setting, so I'm leaving it. Now waiting on the server agent.",
            "  Done.",
        ]);
        assert_eq!(reflow(&lines(&text), 80, 0), want);
    }

    #[test]
    fn two_short_hard_broken_lines_never_join() {
        // Alone, they never came near the width.
        let short = lines(&["  Ran 2 shell commands", "  Read 3 files, listed 1 directory"]);
        assert_eq!(reflow(&short, 100, 0), short);
        // After a wrapped paragraph, each is compared with the line before it and ends early.
        let mut text = README.to_vec();
        text.extend(["  Tests: 6 passed", "  Lint: clean"]);
        let want = lines(&[
            "  The README is done. It now has a short \"Rate limit\" subsection under the API table and a note on the headers.",
            "  Tests: 6 passed",
            "  Lint: clean",
        ]);
        assert_eq!(reflow(&lines(&text), 80, 0), want);
    }

    #[test]
    fn text_that_never_reached_the_width_joins_nothing() {
        // Wrapped at 58 cells: a wrap in a 60-column terminal, a break in a 100-column one.
        let text = lines(&[
            "Retries are spread with full jitter, so a burst of refused",
            "clients does not come back at the same moment and get",
            "refused again.",
        ]);
        assert_eq!(reflow(&text, 100, 0), text);
        assert_eq!(reflow(&text, 60, 0), "Retries are spread with full jitter, so a burst of refused clients does not come back at the same moment and get refused again.");
    }

    #[test]
    fn paragraphs_separated_by_a_blank_line_stay_separate() {
        let mut text = README.to_vec();
        text.push("");
        text.extend(DECISION);
        let want = lines(&[
            "  The README is done. It now has a short \"Rate limit\" subsection under the API table and a note on the headers.",
            "",
            "  Decision for you: the limiter keeps its counts in memory, so a restart forgets them and two server processes \
             count separately. That is fine for one box; for more, say so and I'll move the counts to Redis.",
        ]);
        assert_eq!(reflow(&lines(&text), 80, 0), want);
        // A blank line of spaces is a blank line too.
        let spaced = lines(&[README[0], README[1], "  ", DECISION[0]]);
        assert_eq!(reflow(&spaced, 80, 0), lines(&[&reflow(&lines(&README), 80, 0), "  ", DECISION[0]]));
    }

    #[test]
    fn a_frame_after_a_full_width_line_is_left_as_it_is() {
        let top = format!("╭{}╮", "─".repeat(58));
        let mid = format!("│ ✻ Welcome to Claude Code!{}│", " ".repeat(32));
        let bottom = format!("╰{}╯", "─".repeat(58));
        let text = lines(&["The limiter, its routes and the README are all in place now.", &top, &mid, &bottom]);
        assert_eq!([&top, &mid, &bottom].map(|s| cells(s)), [60, 60, 60]);
        assert_eq!(reflow(&text, 60, 0), text);
        // A table indented under a message is a frame too, whichever side it is on.
        let table = lines(&["  │ hit(key) │ counts one request for the key and says whether it is allowed │", "  of the window."]);
        assert_eq!(reflow(&table, 80, 0), table);
    }

    #[test]
    fn a_code_block_of_short_lines_is_left_as_it_is() {
        let text = lines(&[
            "  ```js",
            "  const limiter = new RateLimiter({ limit: 10, windowMs: 60_000 });",
            "  app.use((req, res, next) => {",
            "    if (!limiter.hit(req.ip)) return res.status(429).end();",
            "    next();",
            "  });",
            "  ```",
        ]);
        assert_eq!(reflow(&text, 80, 0), text);
    }

    #[test]
    fn a_url_broken_mid_word_at_the_width_joins_without_a_space() {
        let text = lines(&["Docs: https://github.com/csarkosh/ide-arkos/blob/main/docs/g", "uide.md#keys lists them."]);
        assert_eq!(cells("Docs: https://github.com/csarkosh/ide-arkos/blob/main/docs/g"), 60);
        assert_eq!(reflow(&text, 60, 0), "Docs: https://github.com/csarkosh/ide-arkos/blob/main/docs/guide.md#keys lists them.");
    }

    #[test]
    fn an_unindented_paragraph_whose_widest_line_is_the_first_joins_with_a_space() {
        let text = lines(&["Each retry waits a random time between zero and the backoff,", "so a burst of refused clients does not come back at once."]);
        assert_eq!(reflow(&text, 60, 0), "Each retry waits a random time between zero and the backoff, so a burst of refused clients does not come back at once.");
    }

    #[test]
    fn lines_wider_than_the_terminal_were_wrapped_by_it_and_their_newlines_are_real() {
        let text = lines(&[
            "2026-10-16T09:14:02.118Z  INFO arkosd::session{id=7 pid=48121}: attached a client, replaying 4096 bytes of scrollback before live output from /bin/zsh",
            "2026-10-16T09:14:02.131Z  INFO arkosd::session{id=7 pid=48121}: resized the pty to 100 columns by 30 rows after the client sent its first frame sizes.",
        ]);
        assert!(text.lines().all(|l| cells(l) == 150));
        assert_eq!(reflow(&text, 100, 0), text);
    }

    #[test]
    fn a_single_line_and_empty_text_are_left_as_they_are() {
        assert_eq!(reflow(BULLET[0], 80, 0), BULLET[0]);
        assert_eq!(reflow(PARAGRAPH[1], 80, 0), PARAGRAPH[1]);
        assert_eq!(reflow("", 80, 0), "");
        assert_eq!(reflow("\n", 80, 0), "\n");
    }

    #[test]
    fn a_trailing_newline_stays_only_if_the_text_had_one() {
        let text = lines(&README) + "\n";
        assert!(reflow(&text, 80, 0).ends_with("headers.\n"));
        assert!(reflow(&lines(&README), 80, 0).ends_with("headers."));
    }

    #[test]
    fn wide_characters_count_as_the_cells_they_take() {
        // 26 characters, 50 cells: by characters this line would be nowhere near the width.
        let first = "● 変更点をまとめました。テストは全部通っています。";
        assert_eq!(cells(first), 50);
        let text = lines(&[first, "  Next: the README."]);
        assert_eq!(reflow(&text, 50, 0), "● 変更点をまとめました。テストは全部通っています。 Next: the README.");
    }

    #[test]
    fn a_selection_that_starts_mid_row_counts_the_cells_before_it() {
        // The selection starts at "the limiter", 20 cells into the Decision paragraph's first row.
        let text = lines(&["the limiter keeps its counts in memory, so a restart forgets", DECISION[1], DECISION[2]]);
        let want = "the limiter keeps its counts in memory, so a restart forgets them and two server processes count separately. \
                    That is fine for one box; for more, say so and I'll move the counts to Redis.";
        assert_eq!(reflow(&text, 80, 20), want);
        // Counted from the left edge instead, the first line would seem to end early.
        assert_ne!(reflow(&text, 80, 0), want);
    }

    #[test]
    fn a_tiny_terminal_never_panics_and_joins_nothing() {
        let text = lines(&BULLET);
        assert_eq!(reflow(&text, 5, 0), text);
        assert_eq!(reflow(&text, 0, 0), text);
        assert_eq!(reflow(&text, 5, usize::MAX), text);
        for s in ["", "\n", "\n\n", " \n ", "a\nb", "日\n本", "  \n\u{301}\n- ", "/\n/"] {
            for cols in [0, 1, 5, 12, 13, 80] {
                let _ = reflow(s, cols, 0);
                let _ = reflow(s, cols, 3);
            }
        }
    }

    #[test]
    fn claude_code_s_prose_reaches_the_width_of_a_wide_terminal() {
        // Claude Code at 160 columns: prose runs to the last column, so the 12-cell margin holds.
        let text = lines(&[
            "● I've started all three agents in parallel: the limiter and its tests, the server wiring with route tests, and the README. Once they've all reported back, I'll",
            "  review the changes, run npm test, and fix anything that fails. One thing to decide while they run: the limiter keeps its counts in memory, so a restart",
            "  forgets them and two server processes count separately.",
        ]);
        assert_eq!(text.lines().map(cells).collect::<Vec<_>>(), [160, 153, 57]);
        let want = "● I've started all three agents in parallel: the limiter and its tests, the server wiring with route tests, and the README. \
                    Once they've all reported back, I'll review the changes, run npm test, and fix anything that fails. One thing to decide while \
                    they run: the limiter keeps its counts in memory, so a restart forgets them and two server processes count separately.";
        assert_eq!(reflow(&text, 160, 0), want);
    }
}
