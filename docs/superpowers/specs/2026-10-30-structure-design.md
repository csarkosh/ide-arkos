# Arkos IDE: folds, the outline and the drag handle (milestone A4)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, section 9); Cyrus asked to start A4 on 2026-10-30.
Builds on A1 to A3, merged at `e16ea3c`.

## 1. Folding headings

- In the rendered view, hovering a heading shows a chevron in the gutter to its left (pointing down
  when open, right when folded); a folded heading always shows its chevron. A click on it folds or
  unfolds the heading: its section (every block after it up to the next heading of its level or
  higher) is hidden. A folded heading ends with a dim "…".
- Fold All and Unfold All, as commands in the palette and as keys in a markdown document,
  `cmd-k cmd-0` and `cmd-k cmd-j` (VS Code's, which Mark Sharp mirrors with its own commands).
- The folds are the view's, never the file's: kept per file in the workspace file as the
  `<details>` state is, keyed by the heading's text and its place among the headings with that text,
  so an edit elsewhere keeps them; they survive a relaunch and are shared by every tab on the file.
- The caret never stands in a hidden block. Up and Down step over a folded section; a move, a find
  hit or a click on the outline that lands in one unfolds the headings hiding it.

## 2. The outline

A third section of the sidebar, "Outline", under the Explorer and above Worktrees, opened and
closed by its header like the others and remembered with them. It lists the headings of the
focused markdown document as a tree (indented by level under their parents):

- A click scrolls the document to the heading (unfolding it if a fold hides it); the caret stays.
- The heading whose section is at the top of the view is highlighted as the document scrolls.
- A heading with headings under it has a chevron that collapses them in the outline (the outline's
  own state, not the document's folds).
- Hovering a row for half a second shows a preview beside the sidebar: the heading and the first
  lines of its section as plain text.
- With no markdown document focused, the section says "No headings".

## 3. The drag handle

Hovering a block in the rendered view shows a six-dot handle in the gutter to its left. Dragging it
moves the whole block (a paragraph, a heading, a whole list, a table, a code block): while dragging,
a 4 px line in the accent colour shows where it will land, between blocks; dropping moves the
block's lines there as one undoable edit, with a blank line kept between it and its neighbours. A
drop on the block itself or the gap right after it changes nothing. A heading's drag moves the
heading alone, not its section (Mark Sharp's draggable blocks are single blocks).

## 4. Layout

The rendered view's rows get a 40 px gutter on the left (20 px today) for the handle (left) and the
chevron (right); the right side keeps 20 px. Raw mode is unchanged.

## 5. Where it lives

`md_fold.rs` (new, pure): the headings of a document, the hidden blocks for a set of folds, the
outline tree with depths, the current heading for a scroll position, the section preview text, and
a block move as an edit. `viewer.rs`: the fold state, its event to the Root for the workspace, the
caret rules, the move. `md_view.rs`: the gutter, the chevron, the handle, hidden rows, the drop line.
`sidebar.rs`, `outline.rs` (new): the section and the preview. `keymap.rs` and the rest: the two
commands. `workspace.rs`: the folds and the outline section's open state.

## 6. Tests and the screen pass

Headless: headings and their sections, nested folds, hidden blocks, fold keys surviving an edit
above, the outline depths, the current heading, the preview text, moves up and down and to the end
with the blank lines kept, a no-op drop, the workspace round trip, the keymap rows. On the local
shoot: hover a heading, fold and unfold it; Fold All; the outline lists the headings, its click
scrolls, its highlight follows; drag a paragraph above a heading.
