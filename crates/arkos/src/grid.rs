//! The grid model for CSV, TSV and spreadsheet files: decoding bytes, parsing delimited text,
//! sniffing the delimiter, column kinds, a cell selection, the text a copy puts on the clipboard,
//! find, and the row and column caps. Everything here is pure; row 0 of a selection is the header.

use std::borrow::Cow;
use std::ops::RangeInclusive;
use std::path::Path;

use crate::viewer::MAX_HITS;

/// Rows a sheet keeps; the rest are dropped and counted in `Sheet::truncated`.
pub const MAX_ROWS: usize = 200_000;
/// Columns a sheet keeps; the rest are dropped and counted in `Sheet::cols_dropped`.
pub const MAX_COLS: usize = 1_000;

/// The byte that separates fields in a delimited file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiter {
    Comma,
    Tab,
    Semicolon,
    Pipe,
}

impl Delimiter {
    /// The delimiter's byte.
    pub fn byte(self) -> u8 {
        match self {
            Delimiter::Comma => b',',
            Delimiter::Tab => b'\t',
            Delimiter::Semicolon => b';',
            Delimiter::Pipe => b'|',
        }
    }
}

const CANDIDATES: [Delimiter; 4] = [Delimiter::Comma, Delimiter::Tab, Delimiter::Semicolon, Delimiter::Pipe];

/// How a column aligns and sorts: `Number` when every non-empty body cell is a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Text,
    Number,
}

/// One table: a header, body rows that may be shorter than it (a missing cell reads as empty), a kind per column, the number the
/// first body row shows, and what the caps dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sheet {
    pub name: String,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub kinds: Vec<ColumnKind>,
    pub first_row: usize,
    pub truncated: Option<usize>,
    pub cols_dropped: usize,
}

/// Where a book came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Delimited(Delimiter),
    Workbook,
}

/// The sheets of one file: a delimited file has one, a workbook one per visible sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub sheets: Vec<Sheet>,
    pub source: Source,
}

/// A cell position; row 0 is the header and body row `i` is row `i + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

impl Sheet {
    /// The column count.
    pub fn cols(&self) -> usize {
        self.header.len()
    }

    /// The text of a cell, empty when the position is outside the sheet.
    pub fn text(&self, cell: Cell) -> &str {
        let row = if cell.row == 0 { Some(&self.header) } else { self.rows.get(cell.row - 1) };
        row.and_then(|r| r.get(cell.col)).map(String::as_str).unwrap_or("")
    }

    /// Builds a sheet from a header and rows: rows past `MAX_ROWS` are dropped and counted, the
    /// width is the widest kept row (header included) cut at `MAX_COLS` with the overflow counted,
    /// the header is padded to the width, and the column kinds are inferred from the rows kept.
    /// Body rows stay ragged: a row is as long as its record, and `text` reads a missing cell as
    /// empty.
    pub fn cap(name: String, mut header: Vec<String>, mut rows: Vec<Vec<String>>) -> Sheet {
        let truncated = (rows.len() > MAX_ROWS).then(|| rows.len() - MAX_ROWS);
        rows.truncate(MAX_ROWS);
        let widest = rows.iter().map(Vec::len).max().unwrap_or(0).max(header.len());
        let width = widest.min(MAX_COLS);
        header.resize(width, String::new());
        for row in &mut rows {
            row.truncate(width);
        }
        let kinds = column_kinds(width, &rows);
        Sheet { name, header, rows, kinds, first_row: 2, truncated, cols_dropped: widest - width }
    }
}

/// Whether the path names a delimited file the grid reads: `.csv` or `.tsv`, any case.
pub fn is_delimited(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv"))
}

/// The bytes as text: a UTF-8 byte order mark is stripped, a UTF-16 one decodes the file as
/// UTF-16 (either order), and anything else is UTF-8 with lossy replacement. Borrowed when the
/// bytes are valid UTF-8 without a mark.
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Cow::Owned(encoding_rs::UTF_16LE.decode_without_bom_handling(&bytes[2..]).0.into_owned());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Cow::Owned(encoding_rs::UTF_16BE.decode_without_bom_handling(&bytes[2..]).0.into_owned());
    }
    String::from_utf8_lossy(bytes)
}

/// The delimiter a file uses. A `.tsv` is tab. Otherwise each candidate is counted outside quotes
/// on the first 20 non-empty records, quoting as `parse` does (only a quote leading a field opens
/// a quoted section): one whose count is the same, and above zero, on every record is consistent,
/// and the consistent one with the most per record wins. With none, comma.
pub fn sniff(text: &str, path: &Path) -> Delimiter {
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("tsv")) {
        return Delimiter::Tab;
    }
    let mut best: Option<(usize, Delimiter)> = None;
    for d in CANDIDATES {
        let counts = record_counts(text.as_bytes(), d.byte());
        let Some(&n) = counts.first() else { continue };
        if n > 0 && counts.iter().all(|&c| c == n) && best.is_none_or(|(m, _)| n > m) {
            best = Some((n, d));
        }
    }
    best.map_or(Delimiter::Comma, |(_, d)| d)
}

/// The delimiter count of each of the first 20 non-empty records, read as `parse` would.
fn record_counts(bytes: &[u8], d: u8) -> Vec<usize> {
    let mut out = Vec::new();
    let (mut count, mut any) = (0, false);
    let (mut in_quotes, mut field_start) = (false, true);
    let mut i = 0;
    while i < bytes.len() && out.len() < 20 {
        let b = bytes[i];
        if in_quotes {
            if b == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 1;
                } else {
                    in_quotes = false;
                }
            }
        } else if b == b'"' && field_start {
            in_quotes = true;
            field_start = false;
            any = true;
        } else if b == b'\n' || b == b'\r' {
            if any {
                out.push(count);
            }
            (count, any, field_start) = (0, false, true);
        } else {
            any = true;
            field_start = b == d;
            if b == d {
                count += 1;
            }
        }
        i += 1;
    }
    if any && out.len() < 20 {
        out.push(count);
    }
    out
}

/// Parses RFC 4180 text: fields split on the delimiter, a field opening with a double quote may
/// hold the delimiter, doubled quotes and line breaks, and records end at `\n`, `\r\n` or a lone
/// `\r`. A quote inside an unquoted field is literal and a final line break adds no record. The
/// first record is the header. The caps apply as it reads: records past the header and
/// `MAX_ROWS` rows, and fields past `MAX_COLS`, are counted and never stored.
pub fn parse(text: &str, delimiter: Delimiter, name: &str) -> Sheet {
    let d = delimiter.byte();
    let bytes = text.as_bytes();
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field: Vec<u8> = Vec::new();
    let mut fields = 0usize; // fields read in the current record, stored or not
    let mut widest = 0usize; // the most fields in any stored record
    let mut dropped = 0usize; // records past the cap
    let mut in_quotes = false;
    let mut at_start = true; // nothing read yet in the current record
    let mut field_start = true; // nothing read yet in the current field
    let mut i = 0;
    // Ends a field: stored unless the record is past the row cap or the field past the column cap.
    let end_field = |field: &mut Vec<u8>, record: &mut Vec<String>, fields: &mut usize, keep: bool| {
        *fields += 1;
        if keep && record.len() < MAX_COLS {
            record.push(String::from_utf8(std::mem::take(field)).unwrap_or_default());
        } else {
            field.clear();
        }
    };
    while i < bytes.len() {
        let b = bytes[i];
        let keep = records.len() <= MAX_ROWS;
        if in_quotes {
            if b == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    field.push(b'"');
                    i += 1;
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(b);
            }
        } else if b == b'"' && field_start {
            in_quotes = true;
            field_start = false;
            at_start = false;
        } else if b == d {
            end_field(&mut field, &mut record, &mut fields, keep);
            field_start = true;
            at_start = false;
        } else if b == b'\n' || b == b'\r' {
            if b == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            end_field(&mut field, &mut record, &mut fields, keep);
            if keep {
                widest = widest.max(fields);
                records.push(std::mem::take(&mut record));
            } else {
                dropped += 1;
            }
            fields = 0;
            field_start = true;
            at_start = true;
        } else {
            field.push(b);
            field_start = false;
            at_start = false;
        }
        i += 1;
    }
    if !at_start {
        let keep = records.len() <= MAX_ROWS;
        end_field(&mut field, &mut record, &mut fields, keep);
        if keep {
            widest = widest.max(fields);
            records.push(record);
        } else {
            dropped += 1;
        }
    }
    let mut records = records.into_iter();
    let header = records.next().unwrap_or_default();
    let mut sheet = Sheet::cap(name.to_string(), header, records.collect());
    sheet.truncated = (dropped > 0).then_some(dropped);
    sheet.cols_dropped = widest.saturating_sub(MAX_COLS);
    sheet
}

/// A kind for each of `header_len` columns: `Number` when the column has a non-empty body cell
/// and every non-empty one is a number.
pub fn column_kinds(header_len: usize, rows: &[Vec<String>]) -> Vec<ColumnKind> {
    (0..header_len)
        .map(|c| {
            let mut seen = false;
            for cell in rows.iter().filter_map(|r| r.get(c)).filter(|s| !s.trim().is_empty()) {
                if !is_number(cell) {
                    return ColumnKind::Text;
                }
                seen = true;
            }
            if seen { ColumnKind::Number } else { ColumnKind::Text }
        })
        .collect()
}

/// Whether a cell reads as a number: spaces trimmed, one leading `$`, `€` or `£`, an optional
/// sign, digits with well-placed thousands commas and at most one decimal point, an optional
/// trailing `%`. No exponent, no `inf` or `nan`.
pub fn is_number(cell: &str) -> bool {
    let s = cell.trim();
    let s = s.strip_prefix(['$', '€', '£']).unwrap_or(s);
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let s = s.strip_suffix('%').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    if frac.is_some_and(|f| !f.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    if int.is_empty() && frac.is_none_or(str::is_empty) {
        return false;
    }
    if int.contains(',') {
        let mut groups = int.split(',');
        let first = groups.next().unwrap_or("");
        let first_ok = (1..=3).contains(&first.len()) && first.bytes().all(|b| b.is_ascii_digit());
        first_ok && groups.all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()))
    } else {
        int.bytes().all(|b| b.is_ascii_digit())
    }
}

/// What a selection covers beyond its rectangle: whole columns, whole rows or the whole sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Cells,
    Columns,
    Rows,
    All,
}

/// A selection: the rectangle from `anchor` to `head`, widened by `scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cell,
    pub head: Cell,
    pub scope: Scope,
}

impl Selection {
    /// A one-cell selection.
    pub fn cell(c: Cell) -> Self {
        Selection { anchor: c, head: c, scope: Scope::Cells }
    }

    /// The inclusive row and column ranges, whichever way round it was dragged. Columns span
    /// every row, rows every column, and `All` both.
    pub fn rect(&self, sheet: &Sheet) -> (RangeInclusive<usize>, RangeInclusive<usize>) {
        let rows = self.anchor.row.min(self.head.row)..=self.anchor.row.max(self.head.row);
        let cols = self.anchor.col.min(self.head.col)..=self.anchor.col.max(self.head.col);
        let all_rows = 0..=sheet.rows.len();
        let all_cols = 0..=sheet.cols().saturating_sub(1);
        match self.scope {
            Scope::Cells => (rows, cols),
            Scope::Columns => (all_rows, cols),
            Scope::Rows => (rows, all_cols),
            Scope::All => (all_rows, all_cols),
        }
    }

    /// The same selection with the head moved to `to`.
    pub fn extend(&self, to: Cell) -> Self {
        Selection { head: to, ..*self }
    }

    /// A one-cell selection `dr` rows and `dc` columns from the head, clamped to the sheet.
    pub fn move_by(&self, dr: isize, dc: isize, sheet: &Sheet) -> Self {
        let step = |v: usize, d: isize, max: usize| v.saturating_add_signed(d).min(max);
        Selection::cell(Cell {
            row: step(self.head.row, dr, sheet.rows.len()),
            col: step(self.head.col, dc, sheet.cols().saturating_sub(1)),
        })
    }

    /// Whether the cell is inside the selection's rectangle.
    pub fn contains(&self, c: Cell, sheet: &Sheet) -> bool {
        let (rows, cols) = self.rect(sheet);
        rows.contains(&c.row) && cols.contains(&c.col)
    }

    /// The selection with anchor and head brought inside the sheet.
    pub fn clamp(&self, sheet: &Sheet) -> Self {
        let fit = |c: Cell| Cell { row: c.row.min(sheet.rows.len()), col: c.col.min(sheet.cols().saturating_sub(1)) };
        Selection { anchor: fit(self.anchor), head: fit(self.head), scope: self.scope }
    }
}

/// The text a copy puts on the clipboard: the rectangle's cells tab-separated, a record a line,
/// no trailing newline. A cell holding a tab, a line break or a double quote is wrapped in double
/// quotes with its quotes doubled.
// Task 5 (keys, clipboard and the menu) calls this.
#[allow(dead_code)]
pub fn copy_text(sheet: &Sheet, sel: &Selection) -> String {
    let (rows, cols) = sel.rect(sheet);
    let mut out = String::new();
    for r in rows {
        if r != *sel.rect(sheet).0.start() {
            out.push('\n');
        }
        for c in cols.clone() {
            if c != *cols.start() {
                out.push('\t');
            }
            let text = sheet.text(Cell { row: r, col: c });
            if text.contains(['\t', '\n', '\r', '"']) {
                out.push('"');
                out.push_str(&text.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(text);
            }
        }
    }
    out
}

/// The selection's rectangle with the header row added. Column and all-sheet selections already
/// have it.
// Task 5 (keys, clipboard and the menu) calls this.
#[allow(dead_code)]
pub fn with_header(sel: &Selection) -> Selection {
    match sel.scope {
        Scope::Columns | Scope::All => *sel,
        Scope::Cells | Scope::Rows => Selection {
            anchor: Cell { row: 0, col: sel.anchor.col },
            head: Cell { row: sel.anchor.row.max(sel.head.row), col: sel.head.col },
            scope: sel.scope,
        },
    }
}

/// Every cell whose text contains the needle, case-insensitively, in reading order with the
/// header first, at most `MAX_HITS`.
// Task 6 (find over the cells) calls this.
#[allow(dead_code)]
pub fn find(sheet: &Sheet, needle: &str) -> Vec<Cell> {
    let needle = needle.to_lowercase();
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    for row in 0..=sheet.rows.len() {
        for col in 0..sheet.cols() {
            let cell = Cell { row, col };
            if sheet.text(cell).to_lowercase().contains(&needle) {
                hits.push(cell);
                if hits.len() >= MAX_HITS {
                    return hits;
                }
            }
        }
    }
    hits
}

/// The viewer banner for what the caps dropped, `None` when nothing was.
pub fn banner(sheet: &Sheet) -> Option<String> {
    let cols = (sheet.cols_dropped > 0).then(|| format!("{} columns past the {}th", thousands(sheet.cols_dropped), thousands(MAX_COLS)));
    match (sheet.truncated, cols) {
        (Some(n), Some(c)) => Some(format!("first {} rows shown, {} more in the file, and {c}", thousands(MAX_ROWS), thousands(n))),
        (Some(n), None) => Some(format!("first {} rows shown, {} more in the file", thousands(MAX_ROWS), thousands(n))),
        (None, Some(c)) => Some(format!("{c} are not shown")),
        (None, None) => None,
    }
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn csv(text: &str) -> Sheet {
        parse(text, Delimiter::Comma, "t")
    }

    fn sheet(rows: &[&[&str]]) -> Sheet {
        let rows: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect();
        let mut it = rows.into_iter();
        let header = it.next().unwrap_or_default();
        Sheet::cap("t".into(), header, it.collect())
    }

    fn c(row: usize, col: usize) -> Cell {
        Cell { row, col }
    }

    fn sel(a: Cell, h: Cell, scope: Scope) -> Selection {
        Selection { anchor: a, head: h, scope }
    }

    #[test]
    fn a_quoted_field_keeps_its_delimiter_doubled_quotes_and_line_breaks() {
        let s = csv("a,b\n\"x,y\",\"say \"\"hi\"\"\nthen\"\n");
        assert_eq!(s.rows, vec![vec!["x,y".to_string(), "say \"hi\"\nthen".to_string()]]);
    }

    #[test]
    fn crlf_and_a_trailing_line_break_add_no_empty_record() {
        let s = csv("a,b\r\n1,2\r\n");
        assert_eq!(s.header, vec!["a", "b"]);
        assert_eq!(s.rows.len(), 1);
        assert_eq!(csv("a,b\n1,2").rows.len(), 1);
    }

    #[test]
    fn a_lone_cr_ends_a_record() {
        let s = csv("a,b\r1,2\r3,4");
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[1], vec!["3", "4"]);
    }

    #[test]
    fn a_quote_mid_field_is_literal() {
        let s = csv("a,b\n5\" nail,x\"y\n");
        assert_eq!(s.rows[0], vec!["5\" nail", "x\"y"]);
    }

    #[test]
    fn a_short_record_is_padded_and_a_long_one_widens_the_sheet() {
        let s = csv("a,b\n1\n1,2,3\n");
        assert_eq!(s.cols(), 3);
        assert_eq!(s.header, vec!["a", "b", ""]);
        assert_eq!(s.rows, vec![vec!["1"], vec!["1", "2", "3"]]);
        assert_eq!(s.text(c(1, 2)), "");
        assert_eq!(s.text(c(2, 2)), "3");
        assert_eq!(s.kinds.len(), 3);
    }

    #[test]
    fn the_first_record_is_the_header_and_one_record_has_no_rows() {
        let s = csv("a,b");
        assert_eq!(s.header, vec!["a", "b"]);
        assert!(s.rows.is_empty());
        assert_eq!(s.first_row, 2);
        assert_eq!(s.name, "t");
    }

    #[test]
    fn an_empty_file_is_an_empty_sheet() {
        let s = csv("");
        assert!(s.header.is_empty() && s.rows.is_empty() && s.kinds.is_empty());
        assert_eq!(s.cols(), 0);
    }

    #[test]
    fn a_tsv_sniffs_tab_whatever_the_text() {
        assert_eq!(sniff("a,b,c\n1,2,3\n", Path::new("x.TSV")), Delimiter::Tab);
    }

    #[test]
    fn a_semicolon_file_sniffs_semicolon() {
        assert_eq!(sniff("a;b\n1,5;2\n3,5;4\n", Path::new("x.csv")), Delimiter::Semicolon);
    }

    #[test]
    fn semicolons_inside_quotes_do_not_sway_the_sniff() {
        assert_eq!(sniff("a,b\n\"x;y;z\",2\n\"q;r\",3\n", Path::new("x.csv")), Delimiter::Comma);
    }

    #[test]
    fn a_quote_mid_field_does_not_sway_the_sniff() {
        assert_eq!(sniff("a;b\n5\" nail;x\n3\" bolt;y\n", Path::new("x.csv")), Delimiter::Semicolon);
        assert_eq!(sniff("a|b\n\"q|r\"|x\n6\" pipe|y\n", Path::new("x.csv")), Delimiter::Pipe);
    }

    #[test]
    fn a_text_past_the_row_cap_parses_capped_and_counts_the_rest() {
        let text = format!("h\n{}", "x\n".repeat(MAX_ROWS + 5));
        let s = csv(&text);
        assert_eq!(s.rows.len(), MAX_ROWS);
        assert_eq!(s.truncated, Some(5));
        let wide = format!("{}\n", ",".repeat(MAX_COLS + 2));
        let s = csv(&wide);
        assert_eq!(s.cols(), MAX_COLS);
        assert_eq!(s.cols_dropped, 3);
    }

    #[test]
    fn a_dropped_row_does_not_widen_the_kept_ones() {
        let mut rows: Vec<Vec<String>> = (0..MAX_ROWS).map(|_| vec!["1".to_string()]).collect();
        rows.push(vec!["1".to_string(); 5]);
        let s = Sheet::cap("t".into(), vec!["h".into()], rows);
        assert_eq!(s.cols(), 1);
        assert_eq!(s.cols_dropped, 0);
    }

    #[test]
    fn a_one_column_file_sniffs_comma() {
        assert_eq!(sniff("a\nb\nc\n", Path::new("x.csv")), Delimiter::Comma);
        assert_eq!(sniff("", Path::new("x.csv")), Delimiter::Comma);
    }

    #[test]
    fn the_consistent_delimiter_wins_and_a_tie_goes_to_the_most_per_record() {
        // Commas vary, pipes are steady.
        assert_eq!(sniff("a|b,c\n1|2\n3|4,5,6\n", Path::new("x.csv")), Delimiter::Pipe);
        // Both steady: the one with more per record.
        assert_eq!(sniff("a,b,c;d\n1,2,3;4\n", Path::new("x.csv")), Delimiter::Comma);
        assert_eq!(sniff("a,b;c;d\n1,2;3;4\n", Path::new("x.csv")), Delimiter::Semicolon);
    }

    #[test]
    fn a_utf8_bom_is_stripped_and_utf16_decodes_by_its_bom() {
        assert_eq!(decode(b"\xEF\xBB\xBFa,b"), "a,b");
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("a,é".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&le), "a,é");
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain("a,é".encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(decode(&be), "a,é");
        assert!(matches!(decode(b"plain"), Cow::Borrowed("plain")));
    }

    #[test]
    fn other_bytes_decode_as_utf8_lossy() {
        assert_eq!(decode(b"caf\xE9"), "caf\u{FFFD}");
    }

    #[test]
    fn a_column_of_currency_percent_and_thousands_is_a_number_column() {
        for ok in ["$1,234.50", "€5", "£7", "-3", "+3.5", "12%", "1,234.5%", " 42 ", ".5", "1,000,000"] {
            assert!(is_number(ok), "{ok}");
        }
        for bad in ["1.2.3", "12abc", "", "-", "$", "1e5", "inf", "nan", "1,23", "1,2345", ",5", "%"] {
            assert!(!is_number(bad), "{bad}");
        }
        let s = csv("a\n\"$1,000\"\n12%\n-4.5\n");
        assert_eq!(s.kinds, vec![ColumnKind::Number]);
    }

    #[test]
    fn a_blank_cell_does_not_count_and_a_word_makes_the_column_text() {
        let s = csv("a,b,c\n1,x,\n,2,\n3,2,\n");
        assert_eq!(s.kinds, vec![ColumnKind::Number, ColumnKind::Text, ColumnKind::Text]);
    }

    #[test]
    fn a_selection_s_rect_is_the_same_whichever_way_it_was_dragged() {
        let s = sheet(&[&["a", "b", "c"], &["1", "2", "3"], &["4", "5", "6"]]);
        let one = sel(c(2, 0), c(1, 2), Scope::Cells).rect(&s);
        let two = sel(c(1, 2), c(2, 0), Scope::Cells).rect(&s);
        assert_eq!(one, two);
        assert_eq!(one, (1..=2, 0..=2));
        assert_eq!(sel(c(1, 1), c(1, 1), Scope::Cells).extend(c(2, 2)).rect(&s), (1..=2, 1..=2));
    }

    #[test]
    fn move_by_clamps_at_the_edges_and_columns_rows_and_all_span_the_sheet() {
        let s = sheet(&[&["a", "b", "c"], &["1", "2", "3"], &["4", "5", "6"]]);
        let m = Selection::cell(c(0, 0)).move_by(-1, -1, &s);
        assert_eq!((m.head, m.scope), (c(0, 0), Scope::Cells));
        assert_eq!(Selection::cell(c(2, 2)).move_by(5, 5, &s).head, c(2, 2));
        assert_eq!(Selection::cell(c(1, 1)).move_by(1, -1, &s).head, c(2, 0));
        assert_eq!(sel(c(1, 2), c(1, 0), Scope::Columns).rect(&s), (0..=2, 0..=2));
        assert_eq!(sel(c(1, 1), c(2, 1), Scope::Rows).rect(&s), (1..=2, 0..=2));
        assert_eq!(sel(c(1, 1), c(1, 1), Scope::All).rect(&s), (0..=2, 0..=2));
        assert!(sel(c(1, 1), c(2, 1), Scope::Rows).contains(c(2, 0), &s));
        assert!(!Selection::cell(c(1, 1)).contains(c(1, 2), &s));
        assert_eq!(Selection::cell(c(9, 9)).clamp(&s).head, c(2, 2));
    }

    #[test]
    fn copy_text_quotes_a_tab_a_line_break_and_a_quote_and_includes_the_header_from_row_0() {
        let s = sheet(&[&["h1", "h2"], &["a\tb", "say \"hi\""], &["x\ny", "plain"]]);
        let all = copy_text(&s, &sel(c(0, 0), c(2, 1), Scope::Cells));
        assert_eq!(all, "h1\th2\n\"a\tb\"\t\"say \"\"hi\"\"\"\n\"x\ny\"\tplain");
        assert_eq!(copy_text(&s, &Selection::cell(c(2, 1))), "plain");
        assert_eq!(copy_text(&s, &sel(c(1, 0), c(2, 0), Scope::Cells)), "\"a\tb\"\n\"x\ny\"");
    }

    #[test]
    fn a_column_copied_from_its_header_carries_its_name() {
        let s = sheet(&[&["h1", "h2"], &["a", "b"], &["c", "d"]]);
        assert_eq!(copy_text(&s, &sel(c(0, 1), c(0, 1), Scope::Columns)), "h2\nb\nd");
        let w = with_header(&sel(c(2, 1), c(1, 1), Scope::Cells));
        assert_eq!(copy_text(&s, &w), "h2\nb\nd");
    }

    #[test]
    fn find_is_case_insensitive_in_reading_order_and_capped() {
        let s = sheet(&[&["Name", "x"], &["ANNA", "b"], &["c", "nAn"]]);
        assert_eq!(find(&s, "nA"), vec![c(0, 0), c(1, 0), c(2, 1)]);
        assert!(find(&s, "zzz").is_empty());
        let rows: Vec<Vec<String>> = (0..MAX_HITS + 5).map(|_| vec!["hit".to_string()]).collect();
        let big = Sheet::cap("t".into(), vec!["h".into()], rows);
        assert_eq!(find(&big, "hit").len(), MAX_HITS);
    }

    #[test]
    fn rows_past_the_cap_are_dropped_and_counted() {
        let rows: Vec<Vec<String>> = (0..MAX_ROWS + 7).map(|i| vec![i.to_string()]).collect();
        let s = Sheet::cap("t".into(), vec!["n".into()], rows);
        assert_eq!(s.rows.len(), MAX_ROWS);
        assert_eq!(s.truncated, Some(7));
        assert_eq!(s.kinds, vec![ColumnKind::Number]);
        assert_eq!(csv("a\n1\n").truncated, None);
    }

    #[test]
    fn columns_past_the_cap_are_dropped_and_counted() {
        let wide: Vec<String> = (0..MAX_COLS + 3).map(|i| i.to_string()).collect();
        let s = Sheet::cap("t".into(), vec!["h".into()], vec![wide]);
        assert_eq!(s.cols(), MAX_COLS);
        assert_eq!(s.cols_dropped, 3);
        assert_eq!(s.rows[0].len(), MAX_COLS);
        assert_eq!(s.kinds.len(), MAX_COLS);
    }

    #[test]
    fn the_banner_names_the_rows_and_columns_dropped() {
        let mut s = csv("a\n1\n");
        assert_eq!(banner(&s), None);
        s.truncated = Some(31_214);
        assert_eq!(banner(&s).unwrap(), "first 200,000 rows shown, 31,214 more in the file");
        s.cols_dropped = 24;
        assert_eq!(banner(&s).unwrap(), "first 200,000 rows shown, 31,214 more in the file, and 24 columns past the 1,000th");
        s.truncated = None;
        assert_eq!(banner(&s).unwrap(), "24 columns past the 1,000th are not shown");
    }

    fn write_csv(grid: &[Vec<String>]) -> String {
        let mut out = String::new();
        for row in grid {
            let line: Vec<String> = row
                .iter()
                .map(|cell| {
                    if cell.is_empty() || cell.contains([',', '"', '\n', '\r', '\t']) {
                        format!("\"{}\"", cell.replace('"', "\"\""))
                    } else {
                        cell.clone()
                    }
                })
                .collect();
            out.push_str(&line.join(","));
            out.push('\n');
        }
        out
    }

    proptest! {
        #[test]
        fn any_grid_written_as_csv_parses_back_to_itself(
            grid in (1..5usize).prop_flat_map(|w| {
                proptest::collection::vec(proptest::collection::vec("[a-z\",\t\n\r ]{0,6}", w), 1..6)
            })
        ) {
            let s = parse(&write_csv(&grid), Delimiter::Comma, "t");
            let mut back = vec![s.header.clone()];
            back.extend(s.rows.clone());
            prop_assert_eq!(back, grid);
        }
    }
}
