# Images and links (milestone A7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-10-30-images-links-design.md`

## Progress (2026-10-30)

Done on branch `links`:

1. `md_links` (new): GitHub anchors with repeats, the heading an anchor names, the workspace root and the name lookup (beside, `.md`, breadth first under the workspace, skipping hidden folders, `target` and `node_modules`), the image folder pattern, the free `image-N` name, a path written relative and bracketed, where a dropped file goes and its markdown (tests in the module).
2. Wikilinks: the parser option on, a wikilink's address its name (tests in `md_blocks` and `md_reveal`).
3. `link_action`: `Anchor` for `#x`, the anchor kept on `OpenMarkdown`, the lookup when a path names no file; the viewer goes to an anchor now or after its load (`pending_anchor`, and `Root::pending_anchors` for a viewer not made yet).
4. Images in: paste of a clipboard image, drop of Finder files on the block drop targets, both as paragraphs of their own (`paragraph_spot`, tested); `viewer.image_destination` in the config, the CLI, the telemetry allowlist and the docs.
5. The screen pass: `#details` followed, `[[other|the other note]]` followed, `other.md#part-two` opened scrolled there, an image pasted (first written mid-paragraph, where Arkos draws only its alt text: now its own paragraph). The Finder drop is not driven by the shoot, which cannot drag from Finder; its placement is the tested helper the paste uses.
