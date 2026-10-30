# Typing rules (milestone A2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enter, Shift+Enter, Tab, Shift+Tab, Cmd+B and Cmd+I shape a markdown document as Mark Sharp does; pasting an address over a selection makes a link; a right-click opens Cut, Copy, Paste and the formats.

**Architecture:** Every rule is an edit of the text through `edit::Document`, decided by pure functions: `md_format` (new) for the formats, `newline`, `break_line` and `indent_item` in `viewer.rs` for the keys, `paste_link` for the paste. The keymap gains "bold" and "italic" in a `Markdown` key context the viewer pane adds for markdown documents. The menu is a `Transient<MdMenu>` on the Root, drawn as the terminal's link menu is.

**Spec:** `docs/superpowers/specs/2026-10-30-typing-rules-design.md`

## Progress (2026-10-30)

Tasks 1 to 7 are done on branch `typing`. The screen pass ran on the local shoot: Cmd+B on a word
and again, Cmd+I on a selection, the right-click menu and its Bold row, Shift+Enter out of a code
block, Tab, Tab and Shift+Tab under a numbered item, Shift+Enter in an item and on a quote's last
line, Enter twice in a quote and on its empty line, Enter and Shift+Enter in a paragraph, Enter
after a heading. It found two things, fixed: a numbered item nested by Tab rendered as text
(markdown starts a nested numbered list only at 1, so it is renumbered), and ending a quote left a
bare `>` line. The shoot gained a right click, and its clicks no longer carry a Command left over
from a key. Out of scope as the spec says: Cmd+K, Highlight, the selection toolbar, slash commands.

## Global Constraints

- Worktree `~/Projects/ide-arkos/.claude/worktrees/reveal`, branch `typing` off main at `3225710`. `nice -n 15`, never `--release`, one cargo at a time.
- Tests first, headless, named as sentences; no em-dashes in doc comments; no warnings at the end.
- Commit after each task with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- The screen pass runs on the local shoot with throwaway storyboards (see the A1 plan's Progress), deleted afterwards.

### Task 1: `md_format`, the formats as edits
Pure: `toggle(line: &str, sel: Range<usize>, fmt: Format) -> Option<(Range<usize>, String, Range<usize>)>` over one line (bytes), giving the range to replace, its text and the new selection. Formats Bold `**`, Italic `_`, Strike `~~`, Code `` ` ``. Rules of spec section 6; the element under the caret found through `md_blocks::elements` on the line.

### Task 2: Enter and Shift+Enter by block
`KeyAction::BreakLine` for Shift+Enter; `newline` takes the block kind (rendered only) and does section 3; `break_line` does section 4; raw mode keeps a plain break.

### Task 3: Tab and Shift+Tab by levels
`indent_item` moves an item (its marker line and continuation lines) to the content column of the item above, or back to its parent's column (spec section 5).

### Task 4: Cmd+B and Cmd+I
Keymap actions "bold" and "italic" (`cmd-b`, `cmd-i`, context `Markdown`), `Command`s for the palette, the Root dispatching to `Viewer::format`, the viewer pane's key context `Viewer Markdown` for a markdown document, the config comments.

### Task 5: Paste as a link
`paste_link(selected: &str, clip: &str, base_dir: &Path) -> Option<String>` and the Paste path using it for a one-line selection.

### Task 6: The right-click menu
`md_menu.rs`: rows, which are dimmed, run by row; `Root.md_menu: Transient<MdMenu>`; the viewer pane's right press opens it (moving the caret first when the press is outside the selection); dismissed as the other menus.

### Task 7: Docs, the screen pass and the merge
Guide, keyboard table, Status rows; the screen pass of spec section 10 on the local shoot; fixes; merge into main.
