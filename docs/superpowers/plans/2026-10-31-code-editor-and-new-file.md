# Code editor and New File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every text file the user may write is editable in the raw view with its syntax colours kept while typing, and the Explorer's right-click menu creates a file by name.

**Spec:** `docs/superpowers/specs/2026-10-31-code-editor-and-new-file-design.md`

Branch `editor`, worktree `.worktrees/editor`.

## Progress (2026-10-31)

All eight tasks done on the branch. The screen pass on the local shoot (a JavaScript file in the
lantern demo) found three things the tests had not: the input method's field came from the
markdown blocks, so a code document took no typing (the field is now the selection's lines);
`Viewer::rendered` was true for a code document whose tab's mode defaulted to Rendered, so a
click mapped through the rendered view and found nothing (it now needs markdown); and a forced
reload landing after the first load had made the document was dropped as an unchanged file, so a
file opened before the grammars loaded stayed uncoloured (it recolours now). Also fixed on the
way: Escape did not close a document's right-click menu. The shoot's synthetic keystrokes reach
a `LineEdit` as the letter `a` whatever was typed (the viewer gets its text from the input
method and was fine), so the New File names on screen were `aaaaaaaaa`; the name rules are
unit tested.

## Tasks

- [x] 1. `highlight::CodeHighlight`: states per line, `new` (full), `update` (incremental, budgeted), `finish` (the pending tail); tests against a full highlight.
- [x] 2. `viewer::read_file` builds a document and a `CodeHighlight` for a text file that is not markdown; `Loaded.code`, `Viewer.code`; `apply` keeps the document on the same text and takes the spans; `changed` splices the spans through `update`; `reload_all_viewers` includes code documents; `reload_document` passes the highlighter (the viewer keeps an `Arc<Highlighter>` the root hands it).
- [x] 3. `viewer_element`: `code_runs` and the raw view painting a code document's spans; `shape_raw` takes the spans.
- [x] 4. Keys by file kind: `code_newline`, `indent_unit`, Tab, Paste, the slash gate, the `Viewer` key context without `Markdown`, the menu rows.
- [x] 5. `pane_view`: `editable` is any document, the pill only for markdown.
- [x] 6. `sidebar`: "New File" in the menu, the root menu on the empty space, `NewFile` prompt and its row, `new_file_target`, create and open; `Root.new_file`, focus routing, the mouse-down close, `any_motion` and `settle`.
- [x] 7. Docs: guide, development Status rows.
- [x] 9. Delete in the tree's menu (spec 5.3): `Tree::tabs_within`, `Viewer::abandon_edits`, `move_to_trash`, `Explorer::forget`, `Root::trash_path`; docs and a Status row; screen pass deleting an open file and a folder with an open file.
- [x] 8. `cargo test --workspace`, `cargo check --workspace --all-targets` clean, the screen pass on the local shoot.
