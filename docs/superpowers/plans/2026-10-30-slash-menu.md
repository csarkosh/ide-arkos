# Slash menu (milestone A3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Typing `/` opens Mark Sharp's slash menu under the caret; it filters, sizes tables, and inserts lists, headings, tables, code, quotes and alerts, dividers, Mermaid templates, images, footnotes, HTML and equations.

**Spec:** `docs/superpowers/specs/2026-10-30-slash-menu-design.md`

## Progress (2026-10-30)

Done on branch `slash`:

1. `md_slash`: the catalogue, the filter with table sizes, the eighteen Mermaid templates, each entry's edit, the footnote number, the image edit (tests in the module).
2. The viewer: the menu's state, `slash_opens`, closing on a space, the caret leaving or the slash gone, the keys, running an entry as one undo step, the image picker.
3. `slash_menu.rs`: the panel under the slash with hints, chevrons, a submenu panel, ten rows kept around the selection, hover and click. The slash's window point is recorded as its revealed text is laid out (the Root builds the menu before that frame's layout).
4. The A1 fix: a caret on a blank line after a block's text is drawn on an empty row of its own.
5. Docs, and the screen pass on the local shoot: the empty-line caret, the menu and its hints, `/h2`, the Mermaid submenu and Pie rendering, `/3x4`, `/todo` converting an item, Escape keeping the slash. It found that the menu was never drawn (the anchor read before the frame's layout), fixed in step 3.

The menu appears and goes at once, as the caret does; it does not fade.
