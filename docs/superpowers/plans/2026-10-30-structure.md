# Folds, outline and drag handle (milestone A4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-10-30-structure-design.md`

## Progress (2026-10-30)

Done on branch `structure`:

1. `md_fold`: headings and sections, hidden blocks, fold keys, the outline tree and its collapsed rows, the current heading, the preview, moving a block (tests in the module).
2. The viewer: folds saved per file in the workspace (`Workspace::folds`) and shared by the file's tabs, Fold All and Unfold All (keys `cmd-k cmd-0`, `cmd-k cmd-j` in the `Markdown` context), Up and Down over folded sections, a caret or a find hit in one unfolding it, `move_block`.
3. The rendered view: a 40 px gutter, the six-dot handle and the heading chevron shown on hover (a folded heading keeps its chevron and ends with "…"), hidden rows at no height, a transparent top border on each row that turns into the accent line under a dragged block.
4. The sidebar's Outline section between the Explorer and Worktrees, opened and closed and saved like them, with the highlight, the collapse chevrons, click to scroll, and the preview after half a second.
5. The shoot can drag and sends digits with a modifier. The screen pass: fold by chevron, Fold All, an outline click unfolding exactly the folds hiding its heading with the preview, Unfold All, a drag above a heading, and the highlight following Page Down through a long document. It found nothing to fix.
