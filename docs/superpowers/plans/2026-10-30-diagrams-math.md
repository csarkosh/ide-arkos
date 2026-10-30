# Diagrams and math (milestone A9) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-10-30-diagrams-math-design.md`

## Progress (2026-10-30)

Done on branch `diagrams`:

1. `render_view.rs` (new): a `WKWebView` installed under gpui's view in each window (non-persistent store, no background), a render that loads the page, asks for its title until `ok w h` or `error`, sizes itself to the picture, snapshots it at `SCALE` and encodes PNG; ten seconds at most. `mermaid.rs` lost Chrome (`prepare`, `finish`, `size_from_title` instead); the queue, cache, keys and sweep are as they were. `Viewer::start_renders` drives a render on the main thread.
2. `md_math.rs` (new) and `scripts/vendor-mathjax.sh`: MathJax 3.2.2's `tex-svg-full` bundled with its licence, its page (two inline scripts by hash, no menu, no assistive copy), its cache key, and inline TeX as text (tested). `md_blocks`: the math option, inline math as italic runs, a paragraph of display math as `Inline::Math`, math in the reveal tree as emphasis with `$` markers; `md_reveal` treats display math as a picture with its source under it.
3. `md_diagram.rs` (new): zoom steps and the pan kept in the box (tested). `workspace::DiagramMode` saved per file (tested); the viewer's modes, zoom, pan and the Split preview.
4. `md_view`: the header's Diagram, Code and Split, the picture with `−` `+` `fit` on hover and drag to pan, the three modes, the Split preview while editing (drawn after a 400 ms pause), display math centred.
5. The screen pass: a flowchart and display math drawn with no Chrome, a parse error shown, hover controls, zoom twice and pan, Split, a line typed in Split and its picture following, the equation's source under it on a click, inline math revealed. A stray pixel under equations (MathJax's assistive copy) was found and turned off.
