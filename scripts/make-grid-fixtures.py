#!/usr/bin/env python3
"""Writes the spreadsheet fixtures the grid's workbook tests read, into
crates/arkos/tests/fixtures/grid. Standard library only, and deterministic: every zip entry carries
a fixed timestamp, so a rerun gives the same bytes.

Run it as: python3 -I scripts/make-grid-fixtures.py

Writes sheets.xlsx, sheets.ods, empty.xlsx, not-a-workbook.xlsx and wide.xlsx.

legacy.xls is NOT written here. It was exported once from sheets.xlsx with LibreOffice
(/Applications/LibreOffice.app/Contents/MacOS/soffice --headless --convert-to xls --outdir <dir>
sheets.xlsx) and committed. Regenerate it the same way whenever sheets.xlsx changes.
"""
import os
import sys
import zipfile
from xml.sax.saxutils import escape

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "arkos", "tests", "fixtures", "grid")
MAX_COLS = 1000  # grid::MAX_COLS


def letters(i):
    s = ""
    i += 1
    while i:
        i, r = divmod(i - 1, 26)
        s = chr(65 + r) + s
    return s


def write_zip(name, entries):
    """entries: list of (path, text); stored in the order given with a fixed timestamp."""
    with zipfile.ZipFile(os.path.join(OUT, name), "w") as z:
        for path, text in entries:
            info = zipfile.ZipInfo(path, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED if path == "mimetype" else zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            z.writestr(info, text)


def s(ref, text, style=None):
    st = f' s="{style}"' if style is not None else ""
    return f'<c r="{ref}"{st} t="inlineStr"><is><t>{escape(text)}</t></is></c>'


def n(ref, value, style=None):
    st = f' s="{style}"' if style is not None else ""
    return f'<c r="{ref}"{st}><v>{value}</v></c>'


def sheet_xml(rows, merges=()):
    body = "".join(f'<row r="{r}">{"".join(cells)}</row>' for r, cells in rows)
    merge = ""
    if merges:
        merge = f'<mergeCells count="{len(merges)}">' + "".join(f'<mergeCell ref="{m}"/>' for m in merges) + "</mergeCells>"
    return ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
            f"<sheetData>{body}</sheetData>{merge}</worksheet>")


def xlsx_parts(sheets):
    """sheets: list of (name, hidden, xml)."""
    n_sheets = len(sheets)
    ct = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
          '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
          '<Default Extension="xml" ContentType="application/xml"/>'
          '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
          '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
          + "".join(f'<Override PartName="/xl/worksheets/sheet{i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' for i in range(n_sheets))
          + "</Types>")
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
            "</Relationships>")
    wb = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
          '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
          'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'
          + "".join(f'<sheet name="{escape(name)}" sheetId="{i + 1}"{" state=" + chr(34) + "hidden" + chr(34) if hidden else ""} r:id="rId{i + 1}"/>' for i, (name, hidden, _) in enumerate(sheets))
          + "</sheets></workbook>")
    wb_rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
               '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
               + "".join(f'<Relationship Id="rId{i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i + 1}.xml"/>' for i in range(n_sheets))
               + f'<Relationship Id="rId{n_sheets + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
               "</Relationships>")
    # cellXfs: 0 General, 1 date (14), 2 date and time (22), 3 elapsed [h]:mm:ss (46).
    styles = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
              '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
              '<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>'
              '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
              '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
              '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
              '<cellXfs count="4">'
              '<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'
              '<xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>'
              '<xf numFmtId="22" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>'
              '<xf numFmtId="46" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>'
              "</cellXfs></styleSheet>")
    parts = [("[Content_Types].xml", ct), ("_rels/.rels", rels), ("xl/workbook.xml", wb),
             ("xl/_rels/workbook.xml.rels", wb_rels), ("xl/styles.xml", styles)]
    parts += [(f"xl/worksheets/sheet{i + 1}.xml", xml) for i, (_, _, xml) in enumerate(sheets)]
    return parts


def make_sheets_xlsx():
    signups = sheet_xml([
        (1, [s("A1", "Signups 2026")]),
        (2, [s("A2", "Name"), s("B2", "Joined"), s("C2", "Seen"), s("D2", "Count"), s("E2", "Amount"),
             s("F2", "Active"), s("G2", "Doubled"), s("H2", "Ratio"), s("I2", "Elapsed")]),
        (3, [s("A3", "Ada"), n("B3", 46000, 1), n("C3", "46000.5", 2), n("D3", 3), n("E3", "1234567.891"),
             '<c r="F3" t="b"><v>1</v></c>', '<c r="G3"><f>D3*2</f><v>6</v></c>',
             '<c r="H3" t="e"><v>#DIV/0!</v></c>', n("I3", "1.5", 3)]),
        (4, [s("A4", "Grace"), n("B4", 45956, 1), n("C4", "45956.75", 2), n("D4", "0.30000000000000004"),
             n("E4", 1000000000000000), '<c r="F4" t="b"><v>0</v></c>', '<c r="G4"><f>D4*2</f><v>0.6</v></c>',
             n("H4", "0.5"), n("I4", "0.25", 3)]),
    ], merges=["A1:B1"])
    notes = sheet_xml([
        (3, [s("C3", "Left"), s("AB3", "Far")]),
        (4, [n("C4", 1), n("AB4", 2)]),
    ])
    hidden = sheet_xml([(1, [s("A1", "secret")])])
    write_zip("sheets.xlsx", xlsx_parts([("Signups", False, signups), ("Notes", False, notes), ("Hidden", True, hidden)]))


def make_wide_xlsx():
    width = MAX_COLS + 3
    r1 = [s(f"{letters(c)}1", f"h{c}") for c in range(width)]
    r2 = [n(f"{letters(c)}2", c) for c in range(width)]
    write_zip("wide.xlsx", xlsx_parts([("Wide", False, sheet_xml([(1, r1), (2, r2)]))]))


def make_ods():
    mimetype = "application/vnd.oasis.opendocument.spreadsheet"
    manifest = ('<?xml version="1.0" encoding="UTF-8"?>'
                '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">'
                f'<manifest:file-entry manifest:full-path="/" manifest:media-type="{mimetype}"/>'
                '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
                "</manifest:manifest>")

    def text(t):
        return f'<table:table-cell office:value-type="string"><text:p>{escape(t)}</text:p></table:table-cell>'

    def num(v):
        return f'<table:table-cell office:value-type="float" office:value="{v}"><text:p>{v}</text:p></table:table-cell>'

    def date(v):
        return f'<table:table-cell office:value-type="date" office:date-value="{v}"><text:p>{v}</text:p></table:table-cell>'

    def row(*cells):
        return "<table:table-row>" + "".join(cells) + "</table:table-row>"

    content = ('<?xml version="1.0" encoding="UTF-8"?>'
               '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
               'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
               'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">'
               "<office:body><office:spreadsheet>"
               '<table:table table:name="People">' + row(text("Name"), text("Joined"), text("Count"))
               + row(text("Ada"), date("2026-10-26"), num(3)) + row(text("Grace"), date("2025-10-26T14:30:00"), num("2.5"))
               + "</table:table>"
               '<table:table table:name="Second">' + row(text("x"), num(1)) + "</table:table>"
               "</office:spreadsheet></office:body></office:document-content>")
    write_zip("sheets.ods", [("mimetype", mimetype), ("META-INF/manifest.xml", manifest), ("content.xml", content)])


def main():
    os.makedirs(OUT, exist_ok=True)
    make_sheets_xlsx()
    make_wide_xlsx()
    make_ods()
    open(os.path.join(OUT, "empty.xlsx"), "wb").close()
    with open(os.path.join(OUT, "not-a-workbook.xlsx"), "w", newline="") as f:
        f.write("name,joined,count\nAda,2026-10-26,3\nGrace,2025-10-26,5\n")


if __name__ == "__main__":
    sys.exit(main())
