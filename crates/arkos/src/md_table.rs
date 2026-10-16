//! Markdown tables (spec 9, section 3): parsing a table block's source into cells, re-aligning its
//! pipes, and the cell positions the editing keys walk. Everything here is pure and works on the
//! block's source text; positions are char indices within a line, and rows are line indices within
//! the block (the delimiter row is line 1 and never a cell).

use unicode_width::UnicodeWidthStr;

/// A column's alignment, from its delimiter cell: `---` is `Default` (drawn left), `:--` `Left`,
/// `--:` `Right`, `:-:` `Center`. `realign` writes back exactly the colons the source had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Default,
    Left,
    Center,
    Right,
}

/// A parsed table: each column's alignment, the header's cells and the body rows' cells, every
/// row holding exactly the header's count. Cell text is the source's, trimmed, with `\|` kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub align: Vec<Align>,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A cell of a table block: `row` is the source line within the block (0 the header, 2 on the
/// body; the delimiter row, 1, is never a cell) and `cell` the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub row: usize,
    pub cell: usize,
}

/// The table in a block's source: the header row, the delimiter row (every cell `:?-+:?` after
/// trimming, as many cells as the header) and the body rows down to the first blank line. A
/// leading and a trailing pipe are optional and `\|` is a literal pipe; a short row is padded with
/// empty cells and a long one truncated to the header's count. `None` without a well-formed
/// delimiter row.
pub fn parse(source: &str) -> Option<Table> {
    let lines = table_lines(source);
    let [header, delimiter, body @ ..] = lines.as_slice() else { return None };
    let header = cell_texts(header);
    let align = cell_texts(delimiter).iter().map(|c| delimiter_align(c)).collect::<Option<Vec<_>>>()?;
    if align.len() != header.len() {
        return None;
    }
    let rows = body
        .iter()
        .map(|line| {
            let mut cells = cell_texts(line);
            cells.resize(header.len(), String::new());
            cells
        })
        .collect();
    Some(Table { align, header, rows })
}

/// The block's source with its pipes aligned: every cell padded with spaces to its column's display
/// width (on the right for `Default` and `Left`, on the left for `Right`, split for `Center`), one
/// space inside each pipe, a leading and a trailing pipe always, the delimiter row keeping its
/// colons with its dashes stretched to the column (at least three). What followed the table's
/// last line (the trailing newline, the blank lines and the reference definitions the block holds
/// after them, or nothing) is kept as it was. `None` when `parse` fails.
///
/// The grid truncates a long row to the header's count, but `realign` never drops a cell: a short
/// row is padded with empty cells, and a long row's cells past the header's count follow the
/// aligned ones unpadded (trimmed, `\|` kept), the widths coming from the header's columns only.
pub fn realign(source: &str) -> Option<String> {
    let t = parse(source)?;
    let columns = t.header.len();
    // The body rows as typed, every cell kept: `parse` truncated them for the grid.
    let body: Vec<Vec<String>> = table_lines(source)
        .iter()
        .skip(2)
        .map(|line| {
            let mut cells = cell_texts(line);
            if cells.len() < columns {
                cells.resize(columns, String::new());
            }
            cells
        })
        .collect();
    let widths: Vec<usize> = t
        .align
        .iter()
        .enumerate()
        .map(|(c, a)| {
            let colons = match a {
                Align::Default => 0,
                Align::Left | Align::Right => 1,
                Align::Center => 2,
            };
            // The delimiter cell is the column's width plus the two spaces, and holds at least
            // three dashes besides its colons.
            let text = std::iter::once(&t.header).chain(&body).map(|row| row[c].width()).max().unwrap_or(0);
            text.max(1 + colons)
        })
        .collect();
    let row = |cells: &[String]| {
        let aligned = cells.iter().zip(&t.align).zip(&widths).map(|((text, a), &w)| pad(text, *a, w));
        let all: Vec<String> = aligned.chain(cells.iter().skip(columns).cloned()).collect();
        format!("| {} |", all.join(" | "))
    };
    let delimiter: Vec<String> = t
        .align
        .iter()
        .zip(&widths)
        .map(|(a, &w)| match a {
            Align::Default => "-".repeat(w + 2),
            Align::Left => format!(":{}", "-".repeat(w + 1)),
            Align::Right => format!("{}:", "-".repeat(w + 1)),
            Align::Center => format!(":{}:", "-".repeat(w)),
        })
        .collect();
    let mut out = vec![row(&t.header), format!("|{}|", delimiter.join("|"))];
    out.extend(body.iter().map(|r| row(r)));
    Some(out.join("\n") + tail(source))
}

/// The cell holding char `ch` of line `line` of the block: a char on a pipe belongs to the cell on
/// its left, one past the last cell to the last cell. `None` on the delimiter row, below the last
/// row, or when the block is not a table.
pub fn cell_at(source: &str, line: usize, ch: usize) -> Option<CellPos> {
    let (columns, last) = shape(source)?;
    if line == 1 || line > last {
        return None;
    }
    let text = source.split('\n').nth(line)?;
    let cell = segments(text).iter().position(|s| ch <= s.end).unwrap_or(usize::MAX);
    Some(CellPos { row: line, cell: cell.min(columns - 1) })
}

/// Where the caret goes in cell `pos`: its line and the char index of the cell's first character
/// that is not a space, or, for an empty cell, the index after its pipe and one space (clamped to
/// the next pipe). A cell a short row lacks is at the line's end.
pub fn cell_start(source: &str, pos: CellPos) -> Option<(usize, usize)> {
    let text = source.split('\n').nth(pos.row)?;
    let chars: Vec<char> = text.chars().collect();
    let Some(seg) = segments(text).get(pos.cell).cloned() else { return Some((pos.row, chars.len())) };
    let ch = (seg.start..seg.end).find(|&i| !chars[i].is_whitespace()).unwrap_or((seg.start + 1).min(seg.end));
    Some((pos.row, ch))
}

/// The cell after `pos`: the next one in its row, or the next row's first, over the delimiter row.
/// `None` from the last cell of the last row.
pub fn next_cell(source: &str, pos: CellPos) -> Option<CellPos> {
    let (columns, _) = shape(source)?;
    if pos.cell + 1 < columns {
        return Some(CellPos { row: pos.row, cell: pos.cell + 1 });
    }
    row_below(source, pos.row).map(|row| CellPos { row, cell: 0 })
}

/// The cell before `pos`: the previous one in its row, or the row above's last, over the delimiter
/// row. `None` from the header's first cell.
pub fn prev_cell(source: &str, pos: CellPos) -> Option<CellPos> {
    let (columns, _) = shape(source)?;
    if pos.cell > 0 {
        return Some(CellPos { row: pos.row, cell: (pos.cell - 1).min(columns - 1) });
    }
    row_above(pos.row).map(|row| CellPos { row, cell: columns - 1 })
}

/// The same cell in the row above `pos`, over the delimiter row; `None` from the header.
pub fn cell_above(source: &str, pos: CellPos) -> Option<CellPos> {
    shape(source)?;
    row_above(pos.row).map(|row| CellPos { row, cell: pos.cell })
}

/// The same cell in the row below `pos`, over the delimiter row; `None` from the last row.
pub fn cell_below(source: &str, pos: CellPos) -> Option<CellPos> {
    row_below(source, pos.row).map(|row| CellPos { row, cell: pos.cell })
}

/// The text Tab from the last cell inserts after the table's last line (`line_count`): a new row of
/// the header's count of empty cells, `|  |  |` for two columns, so each cell has a caret place
/// after its "| ".
pub fn appended_row(source: &str) -> String {
    let columns = table_lines(source).first().map_or(1, |h| segments(h).len());
    format!("\n| {} |", vec![""; columns].join(" | "))
}

/// How many lines of the block's source the table holds: the lines `realign` rewrites and the grid
/// draws, from the header down to the line before the first blank one (or the last line).
pub fn line_count(source: &str) -> usize {
    table_lines(source).len()
}

/// The table's lines: the block's lines down to the one before its first blank line, where GFM
/// ends a table. The parser gives the lines after it that produce no event of their own (a link
/// reference definition, a `[^1]:` note) to the table block, and they are not the table's.
fn table_lines(source: &str) -> Vec<&str> {
    source.split('\n').take_while(|l| !l.trim().is_empty()).collect()
}

/// What follows the table's last line: its newline, the blank lines and whatever the block holds
/// after them (reference definitions), byte for byte, or nothing.
fn tail(source: &str) -> &str {
    let content = table_lines(source).iter().map(|l| l.len() + 1).sum::<usize>().saturating_sub(1);
    &source[content.min(source.len())..]
}

/// A table block's column count and its last row's line index, when it parses.
fn shape(source: &str) -> Option<(usize, usize)> {
    let t = parse(source)?;
    Some((t.header.len(), 1 + t.rows.len()))
}

/// The row below line `row`, over the delimiter row; `None` from the last row.
fn row_below(source: &str, row: usize) -> Option<usize> {
    let (_, last) = shape(source)?;
    let below = if row == 0 { 2 } else { row + 1 };
    (below <= last).then_some(below)
}

/// The row above line `row`, over the delimiter row; `None` from the header.
fn row_above(row: usize) -> Option<usize> {
    match row {
        0 => None,
        1 | 2 => Some(0),
        r => Some(r - 1),
    }
}

/// The char ranges of a row's cells between its pipes: a pipe after a backslash is text, and a
/// leading and a trailing pipe (the first and last characters that are not spaces) open and close
/// the row rather than separating cells. Each range ends at the pipe after it (or the line's end).
fn segments(line: &str) -> Vec<std::ops::Range<usize>> {
    let chars: Vec<char> = line.chars().collect();
    let pipes: Vec<usize> = (0..chars.len()).filter(|&i| chars[i] == '|' && (i == 0 || chars[i - 1] != '\\')).collect();
    let first = chars.iter().position(|c| !c.is_whitespace());
    let last = chars.iter().rposition(|c| !c.is_whitespace());
    let leading = first.filter(|i| pipes.contains(i));
    let trailing = last.filter(|i| pipes.contains(i) && Some(*i) != leading);
    let open = leading.map_or(0, |i| i + 1);
    let end = trailing.unwrap_or(chars.len());
    let (mut start, mut out) = (open, Vec::new());
    for &p in pipes.iter().filter(|&&p| p >= open && p < end) {
        out.push(start..p);
        start = p + 1;
    }
    out.push(start..end.max(start));
    out
}

/// A row's cells as text, trimmed, with `\|` kept.
fn cell_texts(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    segments(line).into_iter().map(|s| chars[s].iter().collect::<String>().trim().to_string()).collect()
}

/// A delimiter cell's alignment: `:?-+:?`, else `None`.
fn delimiter_align(cell: &str) -> Option<Align> {
    let (left, rest) = cell.strip_prefix(':').map_or((false, cell), |r| (true, r));
    let (right, dashes) = rest.strip_suffix(':').map_or((false, rest), |r| (true, r));
    if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
        return None;
    }
    Some(match (left, right) {
        (false, false) => Align::Default,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (true, true) => Align::Center,
    })
}

/// A cell's text padded with spaces to display width `w` by its column's alignment.
fn pad(text: &str, align: Align, w: usize) -> String {
    let room = w.saturating_sub(text.width());
    let (left, right) = match align {
        Align::Default | Align::Left => (0, room),
        Align::Right => (room, 0),
        Align::Center => (room / 2, room - room / 2),
    };
    format!("{}{text}{}", " ".repeat(left), " ".repeat(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_parses_with_optional_outer_pipes_escaped_pipes_and_ragged_rows() {
        let t = parse("| a | b |\n|:--|--:|\n| 1 | 2 |\nx | y \\| z | extra\n| only |\n").unwrap();
        assert_eq!(t.align, vec![Align::Left, Align::Right]);
        assert_eq!(t.header, vec!["a", "b"]);
        assert_eq!(t.rows, vec![vec!["1", "2"], vec!["x", "y \\| z"], vec!["only", ""]]);
        assert_eq!(parse("| a |\n|---|\n").unwrap().align, vec![Align::Default]);
        assert!(parse("| a |\n| 1 |\n").is_none(), "no delimiter row");
        assert!(parse("| a |\n|-x-|\n").is_none(), "a malformed delimiter row");
        assert!(parse("| a |\n|:|\n").is_none(), "a lone colon is malformed");
        assert_eq!(parse("a|b\n-|-\n").unwrap().header, vec!["a", "b"]);
    }

    #[test]
    fn realign_pads_by_alignment_keeps_the_colons_and_escapes_and_is_idempotent() {
        let src = "|a|bb|c|\n|:-|-:|:-:|\n|ccc|d|e \\| f|\n";
        let out = realign(src).unwrap();
        // The third column is as wide as `e \| f`, six characters, so "c" gets two spaces before
        // and three after.
        assert_eq!(out, "| a   | bb |   c    |\n|:----|---:|:------:|\n| ccc |  d | e \\| f |\n");
        assert_eq!(realign(&out).unwrap(), out);
        assert!(realign("| a |\n| 1 |\n").is_none());
        assert_eq!(realign("| a |\n|---|\n| 1 |").unwrap(), "| a |\n|---|\n| 1 |", "no colon added, no trailing newline added");
        assert_eq!(realign("| é |\n|---|\n| 日本 |\n").unwrap(), "| é    |\n|------|\n| 日本 |\n", "display width, not bytes");
    }

    #[test]
    fn an_escaped_pipe_and_a_missing_trailing_pipe_survive_a_realign_and_a_parse_again() {
        let src = "a | b\n--- | :-:\nx \\| y | z";
        let t = parse(src).unwrap();
        let out = realign(src).unwrap();
        assert_eq!(out, "| a      |  b  |\n|--------|:---:|\n| x \\| y |  z  |");
        assert_eq!(parse(&out).unwrap(), t, "the cells read back as they were");
        assert_eq!(realign(&out).unwrap(), out);
    }

    #[test]
    fn realign_keeps_a_long_row_s_extra_cells_after_the_aligned_ones() {
        let src = "|a|b|\n|-|-|\n|1|22|extra \\| x|\n";
        let out = realign(src).unwrap();
        assert_eq!(out, "| a | b  |\n|---|----|\n| 1 | 22 | extra \\| x |\n");
        assert_eq!(realign(&out).unwrap(), out, "idempotent");
        assert_eq!(parse(&out).unwrap().rows, vec![vec!["1", "22"]], "the grid still shows the header's count");
    }

    #[test]
    fn cells_are_found_and_walked_across_rows_skipping_the_delimiter() {
        let src = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
        assert_eq!(cell_at(src, 0, 2), Some(CellPos { row: 0, cell: 0 }));
        assert_eq!(cell_at(src, 2, 6), Some(CellPos { row: 2, cell: 1 }));
        assert_eq!(cell_at(src, 1, 2), None, "the delimiter row is not a cell");
        assert_eq!(cell_start(src, CellPos { row: 2, cell: 1 }), Some((2, 6)));
        assert_eq!(next_cell(src, CellPos { row: 0, cell: 1 }), Some(CellPos { row: 2, cell: 0 }), "across the row's end, over the delimiter");
        assert_eq!(prev_cell(src, CellPos { row: 2, cell: 0 }), Some(CellPos { row: 0, cell: 1 }));
        assert_eq!(next_cell(src, CellPos { row: 3, cell: 1 }), None);
        assert_eq!(cell_below(src, CellPos { row: 0, cell: 1 }), Some(CellPos { row: 2, cell: 1 }));
        assert_eq!(cell_above(src, CellPos { row: 2, cell: 0 }), Some(CellPos { row: 0, cell: 0 }));
        assert_eq!(cell_above(src, CellPos { row: 0, cell: 0 }), None);
        assert_eq!(appended_row(src), "\n|  |  |");
        let empty = "| a |  |\n|---|---|\n";
        assert_eq!(cell_start(empty, CellPos { row: 0, cell: 1 }), Some((0, 6)), "an empty cell: after the pipe and its space");
    }

    #[test]
    fn a_table_ends_at_its_first_blank_line_and_a_reference_definition_after_it_is_kept() {
        // The parser gives a reference definition no event, so the table block's source runs on
        // to it; GFM ends the table at the blank line.
        let src = "|a|b|\n|---|---|\n|1|2|\n\n[ref]: https://x\n";
        assert_eq!(parse(src).unwrap().rows, vec![vec!["1", "2"]], "one body row, no empty row and no definition row");
        assert_eq!(realign(src).unwrap(), "| a | b |\n|---|---|\n| 1 | 2 |\n\n[ref]: https://x\n", "the definition is kept byte for byte");
        assert_eq!(line_count(src), 3);
        assert_eq!(cell_below(src, CellPos { row: 2, cell: 0 }), None, "Down stops at the last row");
        assert_eq!(next_cell(src, CellPos { row: 2, cell: 1 }), None, "Tab from the last cell appends");
        assert_eq!(cell_at(src, 4, 0), None, "the definition is not a cell");
        let note = "| a |\n|---|\n| 1 |\n\n[^1]: a note\n\n[b]: https://y \"t\"\n";
        assert_eq!(realign(note).unwrap(), note, "an aligned table and the definitions after it are left alone");
    }
}
