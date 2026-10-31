# Code editor and New File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every text file the user may write is editable in the raw view with its syntax colours kept while typing, and the Explorer's right-click menu creates a file by name.

**Spec:** `docs/superpowers/specs/2026-10-31-code-editor-and-new-file-design.md`

Branch `editor`, worktree `.worktrees/editor`.

## Tasks

- [ ] 1. `highlight::CodeHighlight`: states per line, `new` (full), `update` (incremental, budgeted), `finish` (the pending tail); tests against a full highlight.
- [ ] 2. `viewer::read_file` builds a document and a `CodeHighlight` for a text file that is not markdown; `Loaded.code`, `Viewer.code`; `apply` keeps the document on the same text and takes the spans; `changed` splices the spans through `update`; `reload_all_viewers` includes code documents; `reload_document` passes the highlighter (the viewer keeps an `Arc<Highlighter>` the root hands it).
- [ ] 3. `viewer_element`: `code_runs` and the raw view painting a code document's spans; `shape_raw` takes the spans.
- [ ] 4. Keys by file kind: `code_newline`, `indent_unit`, Tab, Paste, the slash gate, the `Viewer` key context without `Markdown`, the menu rows.
- [ ] 5. `pane_view`: `editable` is any document, the pill only for markdown.
- [ ] 6. `sidebar`: "New File" in the menu, the root menu on the empty space, `NewFile` prompt and its row, `new_file_target`, create and open; `Root.new_file`, focus routing, the mouse-down close, `any_motion` and `settle`.
- [ ] 7. Docs: guide, development Status rows.
- [ ] 8. `cargo test --workspace`, `cargo check --workspace --all-targets` clean, the screen pass on the local shoot.
