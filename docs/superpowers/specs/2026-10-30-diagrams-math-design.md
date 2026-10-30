# Arkos IDE: diagrams and math (milestone A9)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, sections 3.6, 3.8 and 9, ruling 5); Cyrus asked to start
A9 on 2026-10-30. Builds on A1 to A8 (merged at `310f149`). The last milestone of the programme.

## 1. Diagrams drawn in process

Headless Chrome goes. A diagram is drawn by a `WKWebView` of the window's own, never shown: it is
placed in the window's content view far outside its bounds, with a non-persistent data store, and
loads the same page as before (`mermaid::page`: a local file, a content security policy that
allows only the bundled script and the page's own, no network). When the page's title says
`ok <w> <h>`, the view takes a snapshot of that rectangle at `SCALE` times its pixels, and the PNG
and its size go into the cache exactly where Chrome's went, so the cache, its keys, the queue and
the sweep are unchanged. `error <message>` is the diagram's error, as before; a page that has not
answered in ten seconds is an error too. One render at a time per window, on the main thread,
started by the frame that wants it. "Install Google Chrome" is gone: every Mac can draw.

`SCALE` stays 2 (twice the pixels of the CSS size): at 3 the area cap would refuse diagrams that
draw today, and a picture zoomed to 150 percent still reads well.

## 2. A diagram's modes, zoom and pan

- **Modes.** A diagram's header (as a code block's, A5) shows `Diagram`, `Code` and `Split`, the
  current one marked; the choice is saved per file and diagram in the workspace, as an open
  `<details>` section is. Diagram (the default) shows the picture; Code shows the numbered source,
  as a code block; Split shows the source with the picture under it. A click on the picture or the
  source edits the source in place (A5's code block editing); while it is edited, Diagram and Split
  show the picture under the source, drawn again as the text changes.
- **Zoom and pan.** Hovering a picture shows `−`, `+` and `fit` at its top right: steps of 25
  percent from 50 to 300, `fit` back to the block's width. A zoomed picture keeps its box at the
  fitted size and is dragged to pan. Zoom and pan are the view's, not saved.

## 3. Math

- **Blocks.** A paragraph that is only `$$ ... $$` is display math, drawn by MathJax 3.2.2 (the
  `tex-svg` bundle, Apache 2.0, bundled like the mermaid script) in the same hidden view, cached the
  same way, centred. A TeX error shows as the source with MathJax's message, as a diagram's does.
- **Inline.** `$...$` in text (the parser's math option) draws as text: the TeX turned into the
  characters it stands for where Unicode has them (Greek letters, operators and relations, arrows,
  `\sqrt`, single-character and braced super- and subscripts, `\frac{a}{b}` as `a/b`), the rest as
  written, in italic. A picture cannot sit inside a line of gpui text; this keeps the line a line.
- Both reveal their source like any element (A1). `/math` and `/equation` in the slash menu are
  already there (A3).

## 4. Where it lives

`render_view.rs` (new, macOS): the hidden view, the job driver, the snapshot to PNG. `mermaid.rs`:
the Chrome code goes, the page and the cache stay; a math page beside the diagram page. `md_math.rs`
(new, pure): inline TeX to text. `md_blocks`: the math option, `Inline::Math`. `md_view`: the
header with the modes, the zoom overlay and pan, Split, display math. `viewer.rs`, `workspace.rs`:
the modes saved.

## 5. Tests and the screen pass

Headless: the inline TeX conversion, display math found, the modes saved and read, the zoom steps,
the math page. On the local shoot: a flowchart and a sequence diagram drawn with no Chrome running,
the three modes, zoom in and pan, a display equation and inline math, an error in each.
