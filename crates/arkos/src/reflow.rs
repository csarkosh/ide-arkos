//! Copying from a terminal that a program wrapped for its own width. Claude Code wraps its prose
//! and writes each visual line with a real newline, so the selection's text has a break at every
//! row even where the program only ran out of room; alacritty's own join covers only the rows the
//! terminal wrapped. `reflow` tells the program's wraps from the breaks it meant and joins the
//! wraps. Pure, so it is tested.
use unicode_width::UnicodeWidthStr;

/// How far short of the pane Claude Code's input box wraps: its rows hang two cells in under the
/// prompt, so a token wider than that is cut two cells short of the pane.
pub const INPUT_BOX_INSET: usize = 2;

/// Joins the lines a program wrapped for the terminal's width and keeps the ones it broke on
/// purpose. `columns` is the terminal's width, `first_col` the column the text starts at on
/// its first line (a selection that starts mid-row), both in cells. Widths are display widths
/// (`unicode-width`), so CJK and emoji count as the cells they take.
/// A program that cuts at the pane's width: `reflow_at` with `cut_width = columns`.
#[cfg(test)]
pub fn reflow(text: &str, columns: usize, first_col: usize) -> String {
    reflow_at(text, columns, first_col, columns)
}

/// `reflow` for text whose program cuts a token wider than its wrap at `cut_width` cells rather
/// than at the pane's width: Claude Code's input box, two cells short of the pane.
pub fn reflow_at(text: &str, columns: usize, first_col: usize, cut_width: usize) -> String {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let lines: Vec<&str> = body.split('\n').collect();
    // Twelve columns or fewer leave no room to tell a wrap from a break.
    if lines.len() < 2 || columns <= 12 {
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
        // The first line starts `first_col` cells in, so its continuation's indent counts them.
        let a_col = if i == 1 { first_col } else { 0 };
        match join(a.trim_end(), a_col, widths[i - 1], b, b_t, columns, wrap, cut_width) {
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
/// nothing inside a token cut at the program's wrap), or None when `a` ends where its program
/// ended it. `a` is trimmed on the right, starts `a_col` cells in and is `width` cells wide,
/// `b_t` is `b` without its indent, `cut_width` is where the program cuts a token wider than its
/// wrap. The box-drawing, table and rule stops on `a` ignore `a`'s leading spaces, so a table
/// indented under a message counts as one.
fn join(a: &str, a_col: usize, width: usize, b: &str, b_t: &str, columns: usize, wrap: usize, cut_width: usize) -> Option<&'static str> {
    if a.is_empty() || b_t.is_empty() {
        return None;
    }
    let a_t = a.trim_start_matches(' ');
    if starts_structure(b_t) || starts_box(a_t) || starts_box(b_t) {
        return None;
    }
    // Programs that cut their lines at the width rather than wrap them: tables such as `ps aux`,
    // rules such as pytest's headers, and lines cut short with an ellipsis.
    if has_gap(a_t) || has_gap(b_t) || starts_rule(a_t) || starts_rule(b_t) || a.ends_with("...") || a.ends_with('\u{2026}') {
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
    // A program cuts a word only when the word is wider than its wrap: the piece on `a` then
    // ends exactly at the wrap (a line that wrapped on a space ends short of it, the space took
    // the last cell), and the two pieces together are wider than it. `b` continues the word
    // when nothing but the continuation indent comes before it: `a`'s own indent, plus its
    // marker's width when `a` opens an item or a prompt. The two pieces are the same kind of
    // text, unless the word is a URL: a sentence of Chinese or Japanese that fills its row and
    // the Latin word under it are two words.
    let last = a.rsplit(' ').next().unwrap_or("");
    let indent = a_col + a.len() - a_t.len() + item_marker(a_t).unwrap_or(0);
    let rest = match b.get(..indent) {
        Some(lead) if lead.bytes().all(|c| c == b' ') => &b[indent..],
        _ => b,
    };
    let same_kind = last.contains("://") || last.chars().last().is_some_and(|c| c.is_ascii()) == rest.chars().next().is_some_and(|c| c.is_ascii());
    let cut_here = width == cut_width && !rest.starts_with(' ') && same_kind;
    let mid_word = cut_here && (last.width() + next > cut_width || last.contains('/'));
    Some(if mid_word { "" } else { " " })
}

/// The width of the list marker a line opens with, its space included: a bullet, a number, or
/// Claude Code's prompt (`\u{276f}` and a no-break space), whose continuation rows share the indent.
fn item_marker(line: &str) -> Option<usize> {
    const MARKERS: [&str; 10] = ["- ", "* ", "+ ", "\u{2022} ", "\u{25cf} ", "\u{25cb} ", "\u{25d0} ", "\u{23fa} ", "\u{276f} ", "\u{276f}\u{a0}"];
    if let Some(m) = MARKERS.iter().find(|m| line.starts_with(**m)) {
        return Some(m.width());
    }
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    let numbered = rest.len() < line.len() && (rest.starts_with(". ") || rest.starts_with(") "));
    numbered.then(|| line.len() - rest.len() + 2)
}

/// Whether a line opens a list item, a quote, a heading, a code fence or a tool's result in Claude
/// Code (`⎿`), which never continue the line before them.
fn starts_structure(line: &str) -> bool {
    item_marker(line).is_some()
        || line.starts_with("\u{23bf} ")
        || line.starts_with(['>', '#'])
        || line.starts_with("```")
        || line.starts_with("~~~")
        || line.starts_with("[ ]")
        || line.starts_with("[x]")
}

/// Whether a line, without its indent, has a run of two or more spaces before its end: the
/// columns of a table, which a program pads rather than wraps. Two spaces after a sentence's
/// `.`, `!` or `?` are prose, not a column; `ps` pads after letters and digits.
fn has_gap(line: &str) -> bool {
    line.trim_end().match_indices("  ").any(|(i, _)| !line[..i].ends_with(['.', '!', '?', ' ']))
}

/// Whether a line starts with a rule: three or more of `=`, `-`, `_`, `*` or `~` in a row.
fn starts_rule(line: &str) -> bool {
    ["===", "---", "___", "***", "~~~"].iter().any(|r| line.starts_with(r))
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
    fn a_url_broken_at_the_edge_inside_a_claude_code_message_joins_without_a_space() {
        // The continuation's indent is the bullet's width, so past it the URL goes on.
        let opening = lines(&["\u{25cf} See https://github.com/csarkosh/ide-arkos/blob/main/docs/g", "  uide.md#keys for the keys."]);
        assert_eq!(cells("\u{25cf} See https://github.com/csarkosh/ide-arkos/blob/main/docs/g"), 60);
        assert_eq!(reflow(&opening, 60, 0), "\u{25cf} See https://github.com/csarkosh/ide-arkos/blob/main/docs/guide.md#keys for the keys.");
        // A continuation row keeps the same indent as the row before it.
        let continued = lines(&["  theme at https://github.com/csarkosh/ide-arkos/blob/main/d", "  ocs/themes.md."]);
        assert_eq!(cells("  theme at https://github.com/csarkosh/ide-arkos/blob/main/d"), 60);
        assert_eq!(reflow(&continued, 60, 0), "  theme at https://github.com/csarkosh/ide-arkos/blob/main/docs/themes.md.");
        // An indent that does not match the row before it is a new word.
        let deeper = lines(&["  theme at https://github.com/csarkosh/ide-arkos/blob/main/d", "    ocs/themes.md."]);
        assert_eq!(reflow(&deeper, 60, 0), "  theme at https://github.com/csarkosh/ide-arkos/blob/main/d ocs/themes.md.");
    }

    #[test]
    fn a_token_wider_than_the_program_s_wrap_is_cut_there_and_joins_without_a_space() {
        // Claude Code's input box wraps two cells short of the terminal; a token wider than that
        // is cut at its width and the rest goes on under the prompt's indent.
        let text = lines(&["\u{276f}\u{a0}cargo run --features=abcdefghijklmno", "  pqrstuvwxyz0123456789"]);
        assert_eq!(cells("\u{276f}\u{a0}cargo run --features=abcdefghijklmno"), 38);
        assert_eq!(reflow_at(&text, 40, 0, 38), "\u{276f}\u{a0}cargo run --features=abcdefghijklmnopqrstuvwxyz0123456789");
        // Selected from the URL on, so the first line starts two cells in.
        let from_url = lines(&["https://github.com/csarkosh/ide-arko", "  s/actions/runs/39877328412"]);
        assert_eq!(reflow_at(&from_url, 40, 2, 38), "https://github.com/csarkosh/ide-arkos/actions/runs/39877328412");
        // A message cuts at the pane's width: under a bullet's indent, with prose after the token.
        let bullet = lines(&["\u{25cf} Run --token=abcdefghijklmnopqrstuvwxyz", "  0123456789 now."]);
        assert_eq!(cells("\u{25cf} Run --token=abcdefghijklmnopqrstuvwxyz"), 40);
        assert_eq!(reflow(&bullet, 40, 0), "\u{25cf} Run --token=abcdefghijklmnopqrstuvwxyz0123456789 now.");
        // Two cells short of the pane, a message row wrapped on a space: the words stay apart.
        let wrapped = lines(&["\u{25cf} Run --token=abcdefghijklmnopqrstuvwx", "  yz0123456789 now."]);
        assert_eq!(reflow(&wrapped, 40, 0), "\u{25cf} Run --token=abcdefghijklmnopqrstuvwx yz0123456789 now.");
    }

    #[test]
    fn a_full_row_that_ends_on_a_short_word_keeps_its_space() {
        // Two short words across a full row are two words: a program cuts only what is wider
        // than its wrap.
        let text = lines(&["The quick brown fox jumps over the lazy", "dog again"]);
        assert_eq!(cells("The quick brown fox jumps over the lazy"), 39);
        assert_eq!(reflow(&text, 40, 0), "The quick brown fox jumps over the lazy dog again");
    }

    #[test]
    fn a_path_that_ends_short_of_the_terminal_s_edge_is_followed_by_a_space() {
        // The widest line, 70 cells in 80 columns: the program wrapped before the next word, it
        // did not break the path.
        let text = lines(&["The re-flow is pure, so it lives in a module of its own: src/reflow.rs", "for the rules."]);
        assert_eq!(cells("The re-flow is pure, so it lives in a module of its own: src/reflow.rs"), 70);
        assert_eq!(reflow(&text, 80, 0), "The re-flow is pure, so it lives in a module of its own: src/reflow.rs for the rules.");
    }

    #[test]
    fn twelve_columns_or_fewer_join_nothing() {
        let twelve = lines(&["ab cd ef gh", "ij"]);
        assert_eq!(reflow(&twelve, 12, 0), twelve);
        let five = lines(&["ab cd", "ef"]);
        assert_eq!(reflow(&five, 5, 0), five);
        assert_eq!(reflow(&twelve, 13, 0), "ab cd ef gh ij", "13 columns is wide enough to re-flow");
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

    // `ps aux` at 80 columns: ps cuts each row at the terminal's edge, so two of the three fill
    // it, and its columns are runs of spaces.
    const PS_AUX: [&str; 3] = [
        "csarko   48121   2.1  0.3 410933248  52144 s001  S+    9:14AM   0:01.82 node /Us",
        "csarko   48119   0.0  0.1 410822912  10288 s001  Ss    9:14AM   0:00.04 -zsh",
        "root       312   0.0  0.2 410781696  33600   ??  Ss    8:02AM   0:03.11 /usr/lib",
    ];

    #[test]
    fn tabular_output_cut_at_the_width_is_left_as_it_is() {
        assert_eq!(PS_AUX.map(cells), [80, 76, 80]);
        let text = lines(&PS_AUX);
        assert_eq!(reflow(&text, 80, 0), text);
    }

    #[test]
    fn two_spaces_after_a_sentence_are_no_table() {
        let text = lines(&[
            "  The limiter is done.  Next I will wire it into the server routes, with a 429",
            "  and a Retry-After header. Is that the shape you want?  Say so if it is not.",
        ]);
        assert_eq!(text.lines().map(cells).collect::<Vec<_>>(), [78, 77]);
        let want = "  The limiter is done.  Next I will wire it into the server routes, with a 429 \
                    and a Retry-After header. Is that the shape you want?  Say so if it is not.";
        assert_eq!(reflow(&text, 80, 0), want);
    }

    #[test]
    fn a_rule_across_the_width_never_joins_the_line_after_it() {
        let header = format!("{} test session starts {}", "=".repeat(29), "=".repeat(30));
        assert_eq!(cells(&header), 80);
        let text = lines(&[&header, "platform darwin -- Python 3.12.4, pytest-8.2.2, pluggy-1.5.0"]);
        assert_eq!(reflow(&text, 80, 0), text);
        // A rule of any of the usual characters, on either side.
        for c in ["-", "_", "*", "~"] {
            let rule = c.repeat(80);
            let after = lines(&[&rule, "platform darwin -- Python 3.12.4, pytest-8.2.2, pluggy-1.5.0"]);
            assert_eq!(reflow(&after, 80, 0), after, "after a rule of {c}");
        }
        let before = lines(&[DECISION[0], &header]);
        assert_eq!(reflow(&before, 80, 0), before);
    }

    #[test]
    fn a_line_cut_short_with_an_ellipsis_is_a_break() {
        let text = lines(&[
            "FAILED tests/test_limiter.py::test_retry_after_is_a_whole_number - AssertionE...",
            "FAILED tests/test_limiter.py::test_a_zero_limit_refuses_every_request - asser...",
        ]);
        assert!(text.lines().all(|l| cells(l) == 80));
        assert_eq!(reflow(&text, 80, 0), text);
        let unicode = lines(&["FAILED tests/test_limiter.py::test_retry_after_is_a_whole_number - AssertionErr\u{2026}", "FAILED tests/test_limiter.py::test_a_zero_limit_refuses_every_request"]);
        assert_eq!(cells(unicode.lines().next().unwrap()), 80);
        assert_eq!(reflow(&unicode, 80, 0), unicode);
    }

    #[test]
    fn a_tool_result_never_joins_the_bullet_above_it() {
        // Claude Code marks a tool's result with `⎿` under the call's bullet.
        let call = "\u{25cf} Bash(npx jest src/limiter.test.js src/server.test.js --runInBand --ci --bail)";
        assert_eq!(cells(call), 79);
        let text = lines(&[call, "  \u{23bf}  PASS src/limiter.test.js", "     PASS src/server.test.js"]);
        assert_eq!(reflow(&text, 80, 0), text);
        // The marker alone keeps it apart, with one space after it as with two.
        let single = lines(&[call, "  \u{23bf} Tests: 9 passed, 9 total"]);
        assert_eq!(reflow(&single, 80, 0), single);
    }

    #[test]
    fn claude_code_s_prose_reaches_the_width_of_a_wide_terminal() {
        // Claude Code at 160 columns: prose runs to the last column, so the 12-cell margin holds.
        // Typed the way Claude Code wraps, not captured from a session: a real capture at 160
        // columns or more is still owed (Claude Code would not run in a headless pty here).
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
