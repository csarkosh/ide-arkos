# Arkos IDE: tables as a spreadsheet (milestone A6)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, section 9); Cyrus asked to start A6 on 2026-10-30.
Builds on A1 to A5 (merged at `7d9e0c9`) and Tier B's tables (grid, re-align on leave).

## 1. The model

A table in the rendered view stays a grid while the caret is in it (today it turns into its pipes).
The text stays the truth: every change is an edit of the table's lines, and leaving the table
re-aligns its pipes as Tier B does.

- **A selected cell.** A click on a cell selects it: a ring in the accent colour. Typing replaces
  its text and starts editing it. Enter moves down (a new row from the last), Tab and Shift+Tab to
  the next and previous cell (a new row from the last cell), the arrows to the neighbouring cell
  (Up from the header and Down from the last row leave the table). F2, or a double click, edits
  the cell with the caret after its text. Delete clears it. Cmd+C copies its text, Cmd+X cuts it,
  Cmd+V puts the clipboard's first line in it. Escape leaves the table.
- **An editing cell.** The cell shows its markdown as written, with the caret; Left, Right and
  Cmd+Left, Cmd+Right move within it, a typed `|` goes in as `\|`, Delete never reaches a pipe.
  Enter and Up, Down move to the cell below or above, Tab to the next, Escape back to selecting
  the cell.
- **Rows, columns, the table.** A click on a row's left edge selects the row, on a column's top
  edge the column, on the top-left corner the whole table; Delete clears what is selected.
- **Growing.** Hovering the grid's right edge or its bottom edge shows a "+" strip; a click adds a
  column or a row and selects its first cell.
- **The menu.** A right-click on a cell selects it and opens Cut, Copy, Paste, then Insert Row
  Above, Insert Row Below, Insert Column Left, Insert Column Right, Delete Row, Delete Column,
  Delete Table, then Align Left, Align Center, Align Right (the column's delimiter row gets its
  colons). Deleting the header row makes the first body row the header; deleting the last column
  or the only row deletes the table.

Every structural change is one undoable edit that rewrites the table aligned. A table that does not
parse stays its source, as today. Raw mode is unchanged.

## 2. Where it lives

`md_table.rs`: a cell's text range, setting and clearing cells, inserting and deleting rows and
columns, alignment, the table rebuilt aligned (pure, tested). `viewer.rs`: the table mode (selected
or editing), the pick (row, column, all), the keys, the clipboard, the operations. `md_view.rs`: the
grid drawn active (the ring, the editing cell and its caret, the pick's tint, the edge selectors, the
"+" strips). `md_menu.rs`: the table rows.

## 3. Tests and the screen pass

Headless: every operation on a table's source (with escaped pipes and alignment kept, the tail kept),
the cell ranges, the header deletion rule, the keys' cell moves at the edges, typing replacing and
escaping. On the local shoot: click a cell, type, Enter, Tab to a new row, F2 and edit, the "+"
strips, a column selected and cleared, the menu's Insert Row Below and Align Center, Delete Column.
