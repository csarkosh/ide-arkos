//! A history wrapped again at word boundaries, for a pane that changed width under a program
//! that writes prose down the main screen (Claude Code's classic renderer).
//!
//! A terminal's own reflow cuts a row wherever the new width falls and starts the rest at column
//! 0: "mostly su" / "nny". Claude Code wraps at words under an indent, so a history reflowed that
//! way reads as broken (Cyrus, 2026-11-17: "why is the indentation weird after I change the width
//! back and forth?", then "re-wrap the history at word boundaries too"). Here each line is
//! wrapped after the last space that fits, its later rows under the line's own indent (under the
//! text after a bullet or a number), and a token wider than the pane is still cut where the width
//! falls.
//!
//! A wrapped line is marked so that it can be put back together: every row but its last ends in
//! a cell flagged `WRAPLINE`, as a terminal's own wrapped rows do; where the break took the place
//! of a space that cell is a hidden space, and the indent of a later row is hidden spaces too. So
//! a pane made narrow and wide again shows the rows it had.
//!
//! Claude Code ends every row it wraps with a newline, so to a terminal each of its rows is a
//! line, and a paragraph written in a narrow pane would stay a narrow column in a wide one. So
//! rows not yet looked at are first joined into the lines their program wrapped them from, by
//! the rule a copy uses (`reflow::join`), at the width they were written for: that is the grid's
//! width, since every change of width comes through here. A row that has been through here is
//! marked (`SETTLED`), because the rule depends on the width a row was written for and would
//! tell a different tale at another.

use alacritty_terminal::grid::GridCell;
use alacritty_terminal::term::cell::{Cell, Flags};
use unicode_width::UnicodeWidthStr;

/// On the first cell of every row written here: its line has been told from its neighbours, at
/// the width it was written for, and is not looked at again. A bit alacritty leaves unused; a
/// program's own writing replaces the cell and with it the mark.
const SETTLED: Flags = Flags::from_bits_retain(1 << 15);

/// A history wrapped for a new width.
pub struct Rewrapped {
    pub rows: Vec<Vec<Cell>>,
    /// For each line, the row it began at before and the row it begins at now, in order: where
    /// a scrolled view finds the line it was on.
    pub starts: Vec<(usize, usize)>,
}

/// One line as it was written: the cells of its rows put back together, and the row it began at.
struct Line {
    cells: Vec<Cell>,
    first_row: usize,
    /// One row, as its program wrote it and not yet looked at: it may be a row of a paragraph
    /// the program wrapped.
    fresh: bool,
}

/// A space a line may be broken at: not the second half of a wide character.
fn gap(c: &Cell) -> bool {
    c.c == ' ' && !c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
}

/// A hidden space this module put in: a later row's indent, or the break that stands for a space.
fn ours(c: &Cell) -> bool {
    c.c == ' ' && c.flags.contains(Flags::HIDDEN)
}

/// `rows` (each as wide as the grid was) wrapped for `cols` columns.
pub fn rewrap(rows: &[Vec<Cell>], cols: usize) -> Rewrapped {
    let cols = cols.max(2);
    let written = rows.first().map_or(cols, Vec::len);
    let mut out = Rewrapped { rows: Vec::with_capacity(rows.len()), starts: Vec::new() };
    for line in paragraphs(lines(rows), written) {
        let from = out.rows.len();
        out.starts.push((line.first_row, from));
        wrap(&line.cells, cols, &mut out.rows);
        for row in &mut out.rows[from..] {
            row[0].flags.insert(SETTLED);
        }
    }
    out
}

/// A row's text as its program wrote it, without the blank after it.
fn text_of(cells: &[Cell]) -> String {
    let end = cells.iter().rposition(|c| !gap(c)).map_or(0, |i| i + 1);
    cells[..end].iter().filter(|c| !c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)).map(|c| c.c).collect()
}

/// Joins the fresh rows a program wrapped for `columns` into the lines it wrapped them from. A
/// row runs on into the next when the next's first word would not have fitted after it within
/// `columns` (`reflow::join`, which also knows a list item, a table and a rule when it sees one),
/// and when the next sits under the line's own indent, as Claude Code's wrapped rows do. Claude
/// Code wraps at the pane's full width (measured, 2.1.327), so the test is against `columns`
/// itself and not, as a copy's is, against the widest row at hand: a break its program meant is
/// lost for good here, where a copy only pastes one line for two.
fn paragraphs(lines: Vec<Line>, columns: usize) -> Vec<Line> {
    let prose = |l: &Line| l.fresh && l.cells.iter().any(|c| !gap(c));
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    let mut rest = lines.into_iter().peekable();
    while let Some(line) = rest.next() {
        if !prose(&line) {
            out.push(line);
            continue;
        }
        let mut block = vec![line];
        while let Some(next) = rest.next_if(|l| prose(l)) {
            block.push(next);
        }
        let texts: Vec<String> = block.iter().map(|l| text_of(&l.cells)).collect();
        let widths: Vec<usize> = texts.iter().map(|t| t.width()).collect();
        // Twelve columns or fewer leave no room to tell a wrap from a break.
        let wide_enough = columns > 12;
        let mut open: Option<(Line, usize)> = None;
        for (i, line) in block.into_iter().enumerate() {
            let lead = texts[i].chars().take_while(|c| *c == ' ').count();
            let sep = match &open {
                Some((_, hang)) if wide_enough && lead == *hang => crate::reflow::join(&texts[i - 1], 0, widths[i - 1], &texts[i], texts[i].trim_start_matches(' '), columns, columns, columns),
                _ => None,
            };
            // A row that ends exactly at the pane's edge looks cut through a word. It was not when
            // it ends a sentence or a clause, or when the next row opens with an address: a word
            // that happened to fill the row, then a token that had to start a row of its own.
            let sep = sep.map(|sep| if sep.is_empty() && starts_apart(&texts[i - 1], texts[i].trim_start_matches(' ')) { " " } else { sep });
            match (sep, &mut open) {
                (Some(sep), Some((whole, _))) => {
                    // The row's text, the space the wrap stood for (in a band's colour, if the
                    // row had one), then the next row's text without its indent.
                    let end = whole.cells.iter().rposition(|c| !gap(c)).map_or(0, |i| i + 1);
                    let fill = whole.cells.get(end).cloned();
                    whole.cells.truncate(end);
                    if sep == " " {
                        whole.cells.push(fill.unwrap_or_default());
                    }
                    whole.cells.extend(line.cells.into_iter().skip(lead));
                }
                _ => {
                    out.extend(open.take().map(|(whole, _)| whole));
                    let hang = hanging_indent(&line.cells);
                    open = Some((line, hang));
                }
            }
        }
        out.extend(open.map(|(whole, _)| whole));
    }
    out
}

/// Whether `next` opens a word of its own after `row`, though `row` ends at the pane's edge.
fn starts_apart(row: &str, next: &str) -> bool {
    row.ends_with(['.', ',', ';', ':', '!', '?', ')']) || ["http://", "https://", "www."].iter().any(|scheme| next.starts_with(scheme))
}

/// The lines of `rows`: a row that ends in `WRAPLINE` runs on into the next.
fn lines(rows: &[Vec<Cell>]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut open: Option<Line> = None;
    for (i, row) in rows.iter().enumerate() {
        let continued = open.is_some();
        let settled = row.first().is_some_and(|c| c.flags.contains(SETTLED));
        let line = open.get_or_insert_with(|| Line { cells: Vec::new(), first_row: i, fresh: !settled });
        let last = row.last();
        let wraps = last.is_some_and(|c| c.flags.contains(Flags::WRAPLINE));
        let at_space = wraps && last.is_some_and(ours);
        // A later row's indent is ours, not the line's.
        let from = if continued { row.iter().take_while(|c| ours(c) && !c.flags.contains(Flags::WRAPLINE)).count() } else { 0 };
        // More than one row: put together by its marks, not by guessing.
        if continued || wraps {
            line.fresh = false;
        }
        let unmarked: Vec<Cell> = row[from.min(row.len())..]
            .iter()
            .cloned()
            .map(|mut c| {
                c.flags.remove(SETTLED);
                c
            })
            .collect();
        let mut cells = &unmarked[..];
        if at_space {
            // The text, then the one space the break stood for.
            cells = &cells[..cells.len().saturating_sub(1)];
            let text = cells.iter().rposition(|c| !gap(c)).map_or(0, |i| i + 1);
            line.cells.extend(cells[..text].iter().cloned());
            let mut space = last.cloned().unwrap_or_default();
            space.flags = Flags::empty();
            line.cells.push(space);
        } else if wraps {
            // Cut where the width fell: the row's cells run straight on.
            line.cells.extend(cells.iter().filter(|c| !c.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)).cloned().map(|mut c| {
                c.flags.remove(Flags::WRAPLINE);
                c
            }));
        } else {
            let written = cells.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1);
            line.cells.extend(cells[..written].iter().cloned());
            out.extend(open.take());
        }
    }
    out.extend(open);
    out
}

/// The column a line's later rows begin at: under its first row's text, past its indent and past
/// a bullet, a mark or a number that opens it ("- ", "⏺ ", "12. ").
pub fn hanging_indent(cells: &[Cell]) -> usize {
    let lead = cells.iter().take_while(|c| gap(c)).count();
    if lead == cells.len() {
        return 0;
    }
    let token: Vec<char> = cells[lead..].iter().take_while(|c| !gap(c)).map(|c| c.c).collect();
    let n = token.len();
    let mark = n == 1 && !token[0].is_alphanumeric();
    let number = (2..=4).contains(&n) && token[..n - 1].iter().all(char::is_ascii_digit) && matches!(token[n - 1], '.' | ')');
    let text_follows = cells.get(lead + n).is_some_and(gap) && cells[lead + n..].iter().any(|c| !gap(c));
    if (mark || number) && text_follows {
        lead + n + 1
    } else {
        lead
    }
}

/// Wraps one line's cells for `cols` columns onto `out`.
fn wrap(cells: &[Cell], cols: usize, out: &mut Vec<Vec<Cell>>) {
    let text_end = cells.iter().rposition(|c| !gap(c)).map_or(0, |i| i + 1);
    let hang = hanging_indent(cells).min(cols / 2);
    let indent = cells.first().map(|first| Cell { c: ' ', bg: first.bg, flags: Flags::HIDDEN, ..Cell::default() }).unwrap_or_default();
    let mut pos = 0;
    let mut first = true;
    loop {
        let avail = if first { cols } else { cols - hang };
        let mut row: Vec<Cell> = Vec::with_capacity(cols);
        if !first {
            row.resize(hang, indent.clone());
        }
        first = false;
        if text_end - pos <= avail {
            // The rest fits, with as much of the blank after it (a band's fill) as the row holds.
            row.extend_from_slice(&cells[pos..(pos + avail).min(cells.len())]);
            row.resize(cols, Cell::default());
            out.push(row);
            return;
        }
        // After the last space that fits with the text before it; a run of spaces is broken
        // after its first, so the rest opens the next row and nothing is lost.
        let at = (pos + 1..pos + avail).rev().find(|&s| gap(&cells[s]) && !gap(&cells[s - 1]));
        match at {
            Some(space) => {
                row.extend_from_slice(&cells[pos..space]);
                // A band's colour runs on to the pane's edge.
                let mut fill = cells[space].clone();
                fill.flags = Flags::empty();
                row.resize(cols, fill);
                let end = row.last_mut().expect("cols is at least 2");
                end.c = ' ';
                end.flags = Flags::WRAPLINE | Flags::HIDDEN;
                pos = space + 1;
            }
            None => {
                // A token wider than the row: cut where the width falls, never through a wide
                // character.
                let mut take = avail;
                if take > 1 && cells[pos + take - 1].flags.contains(Flags::WIDE_CHAR) {
                    take -= 1;
                }
                row.extend_from_slice(&cells[pos..pos + take]);
                row.resize(cols, Cell { flags: Flags::LEADING_WIDE_CHAR_SPACER, ..Cell::default() });
                row.last_mut().expect("cols is at least 2").flags.insert(Flags::WRAPLINE);
                pos += take;
            }
        }
        out.push(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::{Color, NamedColor};

    /// A row of `cols` cells holding `text`, a wide character taking two.
    fn row(text: &str, cols: usize) -> Vec<Cell> {
        let mut cells = Vec::new();
        for c in text.chars() {
            if c == '漢' || c == '字' {
                cells.push(Cell { c, flags: Flags::WIDE_CHAR, ..Cell::default() });
                cells.push(Cell { c: ' ', flags: Flags::WIDE_CHAR_SPACER, ..Cell::default() });
            } else {
                cells.push(Cell { c, ..Cell::default() });
            }
        }
        assert!(cells.len() <= cols, "{text:?} is wider than {cols}");
        cells.resize(cols, Cell::default());
        cells
    }

    fn text(rows: &[Vec<Cell>]) -> Vec<String> {
        rows.iter().map(|r| r.iter().filter(|c| !c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)).map(|c| c.c).collect::<String>().trim_end().to_string()).collect()
    }

    fn history(lines: &[&str], cols: usize) -> Vec<Vec<Cell>> {
        lines.iter().map(|l| row(l, cols)).collect()
    }

    #[test]
    fn a_line_is_wrapped_after_the_last_word_that_fits_and_goes_on_under_its_text() {
        let rows = history(&["⏺ Today in NYC should be dry and mostly sunny, with a high in the upper 50s."], 80);
        assert_eq!(text(&rewrap(&rows, 40).rows), ["⏺ Today in NYC should be dry and mostly", "  sunny, with a high in the upper 50s."]);
    }

    #[test]
    fn a_bullet_s_later_rows_sit_under_its_text_and_an_indented_row_s_under_its_indent() {
        let rows = history(&["  - National Weather Service (Central Park area): High near 58F, low around", "    46F. The page was not dated, so I cannot confirm it is for today."], 80);
        assert_eq!(
            text(&rewrap(&rows, 40).rows),
            ["  - National Weather Service (Central", "    Park area): High near 58F, low", "    around", "    46F. The page was not dated, so I", "    cannot confirm it is for today."]
        );
        let numbered = history(&["  12. Sign in to the console and open the billing page for the project"], 80);
        assert_eq!(text(&rewrap(&numbered, 40).rows), ["  12. Sign in to the console and open", "      the billing page for the project"]);
    }

    #[test]
    fn a_pane_made_narrow_and_wide_again_shows_the_rows_it_had() {
        let lines = [
            "⏺ Today in NYC should be dry and mostly sunny, with a high in the upper 50s.",
            "",
            "  - National Weather Service (Central Park area): High near 58F, low around",
            "    46F. The page was not dated,  so I cannot confirm it is for today.",
            "  see https://example.com/a/very/long/path/that/no/pane/of/thirty/columns/holds for more",
            "x",
        ];
        let rows = history(&lines, 100);
        for narrow in [70, 40, 30, 17, 9] {
            let once = rewrap(&rows, narrow);
            assert!(once.rows.iter().all(|r| r.len() == narrow));
            assert_eq!(text(&rewrap(&once.rows, 100).rows), lines, "by way of {narrow} columns");
            // And narrowed in two steps as in one.
            assert_eq!(text(&rewrap(&rewrap(&rows, 55).rows, narrow).rows), text(&once.rows), "55 then {narrow}");
        }
    }

    const PARAGRAPH: &str = "⏺ I couldn't get a reliable forecast for NYC today (Tuesday, November 18, 2026). The search results were undated or from other days, so I don't want to give you numbers that might be wrong.";
    const ITEM: &str = "- Possibly today's forecast: A National Weather Service page calls for a high near 63F,";

    #[test]
    fn a_paragraph_claude_code_wrapped_for_a_narrow_pane_fills_a_wider_one() {
        // Cyrus's second screenshot, 2026-11-18: an answer written in a narrow pane stayed a
        // narrow column after the pane widened. Claude Code ends every row with a newline, so
        // its rows are joined into the lines they were wrapped from before they are wrapped again.
        let narrow = history(
            &[
                "⏺ I couldn't get a reliable",
                "  forecast for NYC today",
                "  (Tuesday, November 18, 2026).",
                "  The search results were undated",
                "  or from other days, so I don't",
                "  want to give you numbers that",
                "  might be wrong.",
                "",
                "  What I did find:",
                "",
                "- Possibly today's forecast: A",
                "  National Weather Service page",
                "  calls for a high near 63F,",
                "- Another source: no date",
            ],
            33,
        );
        let unwrapped = history(&[PARAGRAPH, "", "  What I did find:", "", ITEM, "- Another source: no date"], 300);
        for cols in [70, 120, 33, 20] {
            assert_eq!(text(&rewrap(&narrow, cols).rows), text(&rewrap(&unwrapped, cols).rows), "at {cols} columns");
        }
        assert_eq!(text(&rewrap(&narrow, 300).rows), [PARAGRAPH, "", "  What I did find:", "", ITEM, "- Another source: no date"]);
    }

    #[test]
    fn a_word_that_fills_its_row_is_not_run_into_the_address_under_it() {
        // From Cyrus's session, 31 columns: "check" ended at the pane's edge and the address
        // took the next row; joined as a cut word they read "checkhttps://".
        let rows = history(&["  For a current forecast, check", "  https://forecast.weather.gov", "  for New York City."], 31);
        assert_eq!(text(&rewrap(&rows, 80).rows), ["  For a current forecast, check https://forecast.weather.gov for New York City."]);
        // An address cut at the edge is still one address.
        let cut = history(&["  see https://www.accuweather.c", "  om/en/us/new-york for more"], 31);
        assert_eq!(text(&rewrap(&cut, 80).rows), ["  see https://www.accuweather.com/en/us/new-york for more"]);
    }

    #[test]
    fn lines_that_were_never_near_the_pane_s_width_stay_lines() {
        let lines = ["first line", "second line here", "third", "", "    46F. The page was not dated, so I cannot confirm it is for today xx.", "filler under no indent"];
        assert_eq!(text(&rewrap(&history(&lines[..3], 80), 60).rows), lines[..3]);
        // Wide enough to have been wrapped, but the next row does not sit under its indent.
        assert_eq!(text(&rewrap(&history(&lines[4..], 76), 120).rows), lines[4..]);
    }

    #[test]
    fn a_row_told_from_its_neighbours_once_is_not_joined_to_them_at_another_width() {
        // Sixty columns of prose and a short line, written in a pane of a hundred: two lines.
        let a = "The search results were undated or from other days and so on";
        let lines = [a, "word and more"];
        let once = rewrap(&history(&lines, 100), 62);
        assert_eq!(text(&once.rows), lines);
        // At 62 columns the first now nearly fills its row, as a wrapped row would.
        assert_eq!(text(&rewrap(&once.rows, 100).rows), lines);
    }

    #[test]
    fn a_token_wider_than_the_pane_is_cut_where_the_width_falls() {
        let rows = history(&["see https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"], 80);
        let wrapped = rewrap(&rows, 30);
        assert_eq!(text(&wrapped.rows), ["see", "https://example.com/aaaaaaaaaa", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
        assert_eq!(text(&rewrap(&wrapped.rows, 80).rows), ["see https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
    }

    #[test]
    fn a_band_s_fill_is_cut_at_the_pane_s_edge_and_makes_no_row_of_its_own() {
        let band = Color::Named(NamedColor::Blue);
        let mut cells = row("a short message", 80);
        for c in &mut cells {
            c.bg = band;
        }
        let wrapped = rewrap(&[cells], 40);
        assert_eq!(text(&wrapped.rows), ["a short message"]);
        assert!(wrapped.rows[0].iter().all(|c| c.bg == band), "the band reaches the edge");
    }

    #[test]
    fn a_wide_character_is_never_cut_from_its_second_half() {
        let rows = history(&["aaa漢字"], 10);
        let wrapped = rewrap(&rows, 4);
        assert_eq!(text(&wrapped.rows), ["aaa", "漢字"]);
        assert_eq!(text(&rewrap(&wrapped.rows, 10).rows), ["aaa漢字"]);
    }

    #[test]
    fn each_line_is_found_again_by_the_row_it_began_at() {
        let rows = history(&["one two three four five six seven eight nine ten", "short", "eleven twelve thirteen fourteen fifteen sixteen"], 60);
        let wrapped = rewrap(&rows, 20);
        assert_eq!(wrapped.starts, [(0, 0), (1, 3), (2, 4)]);
        let back = rewrap(&wrapped.rows, 60);
        assert_eq!(back.starts, [(0, 0), (3, 1), (4, 2)]);
    }

    #[test]
    fn a_terminal_s_own_wrapped_rows_are_one_line() {
        // A program's long line the terminal wrapped itself: full rows, `WRAPLINE` on the last cell.
        let mut first = row("aaaa bbbb cccc dddd ", 20);
        first[19].flags.insert(Flags::WRAPLINE);
        let rows = vec![first, row("eeee ffff", 20)];
        assert_eq!(text(&rewrap(&rows, 40).rows), ["aaaa bbbb cccc dddd eeee ffff"]);
    }

    /// A tool, not a check: what wrapping a full history costs (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn zz_a_full_history_s_cost() {
        let line = "  - National Weather Service (Central Park area): High near 58F, low around 46F. The page was not dated at all.";
        let rows: Vec<Vec<Cell>> = (0..20_000).map(|_| row(line, 120)).collect();
        let started = std::time::Instant::now();
        let wrapped = rewrap(&rows, 70);
        eprintln!("20,000 rows of 120 columns wrapped to 70: {} rows in {:?}", wrapped.rows.len(), started.elapsed());
    }

    #[test]
    fn the_indent_is_found_past_a_bullet_a_mark_or_a_number() {
        let hang = |s: &str| hanging_indent(&row(s, 60));
        assert_eq!(hang("plain text"), 0);
        assert_eq!(hang("    indented text"), 4);
        assert_eq!(hang("  - a bullet"), 4);
        assert_eq!(hang("⏺ a message"), 2);
        assert_eq!(hang("  3. a step"), 5);
        assert_eq!(hang("a word then more"), 0, "a one-letter word is no mark");
        assert_eq!(hang("-"), 0);
        assert_eq!(hang(""), 0);
    }
}
