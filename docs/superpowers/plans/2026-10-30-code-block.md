# Code block chrome (milestone A5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-10-30-code-block-design.md`

## Progress (2026-10-30)

Done on branch `codeblock`:

1. `md_code`: fences and code lines (fenced, tilde, unclosed, indented), the languages with aliases and filter, the fence edit, the copy text, runs split by line (tests in the module).
2. The caret over fence lines: `snap_pos` steps off them (a new `Dir::Here` for edits and placed carets), `navigate` treats a code block's code lines as its edges and enters at them.
3. The chrome: `code_header` (pill, Copy, a header click selecting the block), numbered rows for the inactive block, and the raw block element's code look (a number column, the block's syntax colours, never bold or italic so glyphs sit where the raw layout puts them) inside the same box for the active one; a caret below the block gets the inactive block and a caret row.
4. The language list (`lang_menu.rs`) under the pill, with search, keys and clicks; Copy with its "Copied" for a second and a half.
5. The screen pass: header and numbers, the active box, Up out, the list on its first click (it waited for the next frame until its anchor asked for one), "py" and Enter, Copy (its label rendered for 1.5 s, confirmed by a render log; the shoot's capture comes later than that), the header click and Delete (the pill's click reached the header too and selected the block, fixed), a Mermaid block clicked into (the caret landed on its hidden fence, fixed) and left with Shift+Enter (on the trailing blank line it wrote a lone backslash, fixed: either key adds a line there).
