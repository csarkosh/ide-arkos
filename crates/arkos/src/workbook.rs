//! Reads spreadsheets (xlsx, xlsm, xlsb, xls, ods) into the grid model through calamine, and turns
//! its cell values into the text Excel's General format would show.

// Nothing outside the tests calls this yet; the grid view that does is a later task, which removes this.
#![allow(dead_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader, SheetVisible, Sheets};

use crate::grid::{Book, Sheet, Source};
use crate::viewer::MAX_PLAIN_BYTES;

/// Whether the path names a spreadsheet the grid reads: `.xlsx`, `.xlsm`, `.xlsb`, `.xls` or
/// `.ods`, any case.
pub fn is_workbook(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["xlsx", "xlsm", "xlsb", "xls", "ods"].iter().any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// Reads the visible, non-empty sheets of a workbook in the workbook's order. A file over
/// `MAX_PLAIN_BYTES` is refused before it is opened, and a panic inside the reader becomes an
/// error. A workbook with nothing to show becomes one empty sheet named after the file. The
/// error is the text the notice shows.
pub fn read(path: &Path) -> Result<Book, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_PLAIN_BYTES {
        return Err(format!(
            "the spreadsheet is {} MB, over the {} MB limit",
            size.div_ceil(1 << 20),
            MAX_PLAIN_BYTES >> 20
        ));
    }
    catch_unwind(AssertUnwindSafe(|| read_unguarded(path)))
        .unwrap_or_else(|_| Err("the spreadsheet could not be read".to_string()))
}

fn read_unguarded(path: &Path) -> Result<Book, String> {
    let mut workbook = open_workbook_auto(path).map_err(|e| e.to_string())?;
    // The 1904 date system is a property of the workbook. Ods dates arrive as ISO text, so it
    // does not apply there.
    let is_1904 = match &workbook {
        Sheets::Xls(x) => x.has_1904_epoch(),
        Sheets::Xlsx(x) => x.has_1904_epoch(),
        Sheets::Xlsb(x) => x.has_1904_epoch(),
        Sheets::Ods(_) => false,
    };
    let visible: Vec<String> = workbook
        .sheets_metadata()
        .iter()
        .filter(|s| s.visible == SheetVisible::Visible)
        .map(|s| s.name.clone())
        .collect();
    let mut sheets = Vec::new();
    for name in visible {
        let range = workbook.worksheet_range(&name).map_err(|e| e.to_string())?;
        let Some((first_row, first_col)) = range.start() else { continue };
        let rows: Vec<Vec<String>> =
            range.rows().map(|row| row.iter().map(|c| cell_text(c, is_1904)).collect()).collect();
        let header = (0..range.width()).map(|i| column_letters(first_col as usize + i)).collect();
        let mut sheet = Sheet::cap(name, header, rows);
        sheet.first_row = first_row as usize + 1;
        sheets.push(sheet);
    }
    if sheets.is_empty() {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut sheet = Sheet::cap(stem, Vec::new(), Vec::new());
        sheet.first_row = 1;
        sheets.push(sheet);
    }
    Ok(Book { sheets, source: Source::Workbook })
}

/// The spreadsheet column name for a zero-based index: 0 is `A`, 25 is `Z`, 26 is `AA`.
pub fn column_letters(index: usize) -> String {
    let mut n = index + 1;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The text a cell shows: dates and durations as dates and durations, numbers the way Excel's
/// General format prints them, booleans as `TRUE` and `FALSE`, errors as their text.
pub fn cell_text(data: &Data, is_1904: bool) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => float_text(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(dt) if dt.is_duration() => duration_text(dt.as_f64()),
        Data::DateTime(dt) => date_text(dt.as_f64(), is_1904),
        Data::DateTimeIso(s) => iso_text(s),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => e.to_string(),
    }
}

/// An ISO 8601 date or date and time as `YYYY-MM-DD` or `YYYY-MM-DD HH:MM`: a midnight time or
/// zero seconds are dropped, as `date_text` does.
fn iso_text(s: &str) -> String {
    let Some((date, time)) = s.split_once('T') else { return s.to_string() };
    let clock = time.get(..5).unwrap_or(time);
    let seconds_zero = time.get(5..).is_none_or(|rest| rest.trim_start_matches([':', '0', '.']).is_empty());
    if clock == "00:00" && seconds_zero {
        date.to_string()
    } else {
        format!("{date} {clock}")
    }
}

/// A float the way Excel's General format shows it: an integer-valued float without a decimal
/// point, otherwise up to 10 significant digits with trailing zeros trimmed, never scientific
/// below 1e15 and never with thousands separators.
pub fn float_text(f: f64) -> String {
    if !f.is_finite() {
        return if f.is_nan() { "NaN".to_string() } else if f > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    let magnitude = f.abs();
    if magnitude >= 1e21 {
        return format!("{f:e}");
    }
    if magnitude == 0.0 {
        return "0".to_string();
    }
    let digits_before = magnitude.log10().floor() as i32 + 1;
    let decimals = (10 - digits_before).clamp(0, 25) as usize;
    let text = format!("{f:.decimals$}");
    let trimmed = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
    if trimmed == "-0" { "0".to_string() } else { trimmed.to_string() }
}

/// A date serial as `YYYY-MM-DD`, or `YYYY-MM-DD HH:MM` when it has a time of day (minutes
/// rounded to the nearest, rolling the day at 24:00).
pub fn date_text(serial: f64, is_1904: bool) -> String {
    let mut days = serial.floor();
    let mut minutes = ((serial - days) * 1440.0).round() as i64;
    if minutes >= 1440 {
        days += 1.0;
        minutes = 0;
    }
    let (y, m, d) = serial_to_civil(days as i64, is_1904);
    if minutes == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", minutes / 60, minutes % 60)
    }
}

/// A duration given in days as `h:mm:ss`, the hours unbounded: 1.5 days is `36:00:00`.
pub fn duration_text(days: f64) -> String {
    let total = (days * 86400.0).round() as i64;
    let sign = if total < 0 { "-" } else { "" };
    let total = total.abs();
    format!("{sign}{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

/// The calendar date (year, month, day) of a whole-day serial. The 1900 system counts a day that
/// never was: serial 60 is the phantom 1900-02-29 and 61 is 1900-03-01, so serials from 61 on
/// are plain days since 1899-12-30 and earlier ones are a day later than that count. The 1904
/// system starts at 1904-01-01 for 0. The conversion is Howard Hinnant's `civil_from_days`.
pub fn serial_to_civil(serial: i64, is_1904: bool) -> (i32, u32, u32) {
    if is_1904 {
        return civil_from_days(serial - 24107);
    }
    match serial {
        60 => (1900, 2, 29),
        s if s > 60 => civil_from_days(s - 25569),
        s => civil_from_days(s - 25569 + 1),
    }
}

/// Howard Hinnant's days-to-civil: days since 1970-01-01 to (year, month, day).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{self, Source};
    use calamine::{CellErrorType, Data, ExcelDateTime, ExcelDateTimeType};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grid").join(name)
    }

    fn book(name: &str) -> grid::Book {
        read(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn names(book: &grid::Book) -> Vec<&str> {
        book.sheets.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn the_fixture_s_sheets_come_in_order_without_the_hidden_one() {
        let b = book("sheets.xlsx");
        assert_eq!(names(&b), ["Signups", "Notes"]);
        assert_eq!(b.source, Source::Workbook);
    }

    #[test]
    fn workbook_extensions_are_recognised_in_any_case() {
        for ok in ["a.xlsx", "a.XLSM", "a.xlsb", "a.xls", "a.Ods"] {
            assert!(is_workbook(Path::new(ok)), "{ok}");
        }
        for no in ["a.csv", "a.xlsx.txt", "xlsx", "a.docx"] {
            assert!(!is_workbook(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn column_letters_pass_z_and_zz() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(51), "AZ");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
    }

    #[test]
    fn a_range_starting_at_c3_numbers_its_rows_from_3_and_its_columns_from_c() {
        let b = book("sheets.xlsx");
        let notes = &b.sheets[1];
        assert_eq!(notes.first_row, 3);
        assert_eq!(notes.header.len(), 26);
        assert_eq!(notes.header[0], "C");
        assert_eq!(notes.header[25], "AB");
        assert_eq!(notes.rows[0][0], "Left");
        assert_eq!(notes.rows[0][25], "Far");
        assert_eq!(notes.rows[1][0], "1");
        assert_eq!(b.sheets[0].first_row, 1);
        assert_eq!(b.sheets[0].header[0], "A");
    }

    #[test]
    fn an_integer_float_a_long_float_and_a_bool_read_as_excel_shows_them() {
        let b = book("sheets.xlsx");
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[2][3], "3");
        assert_eq!(rows[3][3], "0.3");
        assert_eq!(rows[2][4], "1234567.891");
        assert_eq!(rows[3][4], "1000000000000000");
        assert_eq!(rows[2][5], "TRUE");
        assert_eq!(rows[3][5], "FALSE");
        assert_eq!(float_text(3.0), "3");
        assert_eq!(float_text(0.1 + 0.2), "0.3");
        assert_eq!(float_text(1234567.891), "1234567.891");
        assert_eq!(float_text(1e15), "1000000000000000");
        assert_eq!(float_text(-0.0), "0");
        assert_eq!(float_text(-2.5), "-2.5");
        assert_eq!(float_text(0.00000015), "0.00000015");
        assert_eq!(float_text(2.999999999999), "3");
        assert_eq!(float_text(1e21), "1e21");
        assert_eq!(float_text(f64::NAN), "NaN");
    }

    #[test]
    fn a_date_at_midnight_and_one_with_a_time_read_as_dates() {
        let b = book("sheets.xlsx");
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[2][1], "2025-12-09");
        assert_eq!(rows[2][2], "2025-12-09 12:00");
        assert_eq!(rows[3][1], "2025-10-26");
        assert_eq!(rows[3][2], "2025-10-26 18:00");
        assert_eq!(date_text(45956.0, false), "2025-10-26");
        assert_eq!(date_text(45956.5, false), "2025-10-26 12:00");
        assert_eq!(date_text(45956.0 + 14.5 / 24.0, false), "2025-10-26 14:30");
        // Half a minute short of midnight rounds up to 24:00, which rolls the day.
        assert_eq!(date_text(45956.0 + 1439.9 / 1440.0, false), "2025-10-27");
    }

    #[test]
    fn serials_1_59_60_61_25569_and_45956_5_convert() {
        assert_eq!(serial_to_civil(1, false), (1900, 1, 1));
        assert_eq!(serial_to_civil(59, false), (1900, 2, 28));
        assert_eq!(serial_to_civil(60, false), (1900, 2, 29));
        assert_eq!(serial_to_civil(61, false), (1900, 3, 1));
        assert_eq!(serial_to_civil(25569, false), (1970, 1, 1));
        // 45956 is 20387 days after 1970-01-01, which is 2025-10-26 (checked by hand and with
        // Python's datetime); the .5 is noon.
        assert_eq!(serial_to_civil(45956, false), (2025, 10, 26));
        assert_eq!(date_text(45956.5, false), "2025-10-26 12:00");
    }

    #[test]
    fn a_1904_system_serial_is_four_years_later() {
        assert_eq!(serial_to_civil(0, true), (1904, 1, 1));
        assert_eq!(serial_to_civil(1461, true), (1908, 1, 1));
        assert_eq!(serial_to_civil(24107, true), (1970, 1, 1));
        assert_eq!(date_text(1461.5, true), "1908-01-01 12:00");
        // The same serial is 1462 days (four years and a day) apart in the two systems.
        assert_eq!(serial_to_civil(45956, true), (2029, 10, 27));
    }

    #[test]
    fn a_duration_reads_as_h_mm_ss() {
        assert_eq!(duration_text(1.5), "36:00:00");
        assert_eq!(duration_text(0.25), "6:00:00");
        assert_eq!(duration_text(1.0 / 1440.0), "0:01:00");
        assert_eq!(duration_text(0.0), "0:00:00");
        let b = book("sheets.xlsx");
        assert_eq!(b.sheets[0].rows[2][8], "36:00:00");
        assert_eq!(b.sheets[0].rows[3][8], "6:00:00");
    }

    #[test]
    fn an_error_cell_shows_its_text_and_a_formula_its_cached_value() {
        let b = book("sheets.xlsx");
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[2][7], "#DIV/0!");
        assert_eq!(rows[2][6], "6");
        assert_eq!(rows[3][6], "0.6");
        assert_eq!(cell_text(&Data::Error(CellErrorType::NA), false), "#N/A");
    }

    #[test]
    fn cell_text_covers_every_kind_of_value() {
        assert_eq!(cell_text(&Data::Empty, false), "");
        assert_eq!(cell_text(&Data::String("hi".into()), false), "hi");
        assert_eq!(cell_text(&Data::Int(-7), false), "-7");
        assert_eq!(cell_text(&Data::Float(2.5), false), "2.5");
        assert_eq!(cell_text(&Data::Bool(true), false), "TRUE");
        let dt = ExcelDateTime::new(45956.5, ExcelDateTimeType::DateTime, false);
        assert_eq!(cell_text(&Data::DateTime(dt), false), "2025-10-26 12:00");
        let d = ExcelDateTime::new(1.5, ExcelDateTimeType::TimeDelta, false);
        assert_eq!(cell_text(&Data::DateTime(d), false), "36:00:00");
        assert_eq!(cell_text(&Data::DateTimeIso("2025-10-26".into()), false), "2025-10-26");
        assert_eq!(cell_text(&Data::DateTimeIso("2025-10-26T14:30:00".into()), false), "2025-10-26 14:30");
        assert_eq!(cell_text(&Data::DateTimeIso("2025-10-26T00:00:00".into()), false), "2025-10-26");
    }

    #[test]
    fn a_merged_cell_keeps_its_value_top_left() {
        let b = book("sheets.xlsx");
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[0][0], "Signups 2026");
        assert_eq!(rows[0][1], "");
    }

    #[test]
    fn an_ods_reads_too() {
        let b = book("sheets.ods");
        assert_eq!(names(&b), ["People", "Second"]);
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[0], ["Name", "Joined", "Count"]);
        assert_eq!(rows[1], ["Ada", "2026-10-26", "3"]);
        assert_eq!(rows[2], ["Grace", "2025-10-26 14:30", "2.5"]);
    }

    #[test]
    fn an_xls_reads_too() {
        let b = book("legacy.xls");
        assert_eq!(names(&b), ["Signups", "Notes"]);
        let rows = &b.sheets[0].rows;
        assert_eq!(rows[2][0], "Ada");
        assert_eq!(rows[2][1], "2025-12-09");
        assert_eq!(rows[2][3], "3");
        assert_eq!(rows[2][4], "1234567.891");
    }

    #[test]
    fn a_zero_byte_workbook_is_unreadable_not_a_panic() {
        assert!(read(&fixture("empty.xlsx")).is_err());
    }

    #[test]
    fn a_csv_named_xlsx_is_unreadable_with_a_message() {
        let err = read(&fixture("not-a-workbook.xlsx")).unwrap_err();
        assert!(!err.trim().is_empty());
    }

    #[test]
    fn a_workbook_over_the_size_limit_is_refused_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.xlsx");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(crate::viewer::MAX_PLAIN_BYTES + (1 << 20)).unwrap();
        let err = read(&path).unwrap_err();
        assert!(err.contains("65 MB"), "{err}");
    }

    #[test]
    fn a_1904_workbook_s_dates_are_read_on_the_1904_epoch() {
        let b = book("date1904.xlsx");
        assert_eq!(b.sheets[0].rows[0][0], "1904-01-11");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(read(&fixture("nope.xlsx")).is_err());
    }

    #[test]
    fn a_sheet_wider_than_max_cols_is_cut_and_counted() {
        let b = book("wide.xlsx");
        let s = &b.sheets[0];
        assert_eq!(s.header.len(), grid::MAX_COLS);
        assert_eq!(s.cols_dropped, 3);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[0].len(), grid::MAX_COLS);
    }
}
