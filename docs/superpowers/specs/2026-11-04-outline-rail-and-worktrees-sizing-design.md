# The outline moves into the tab, and the Worktrees section grows and shows status

Status: design, 2026-11-04. Asked for by Cyrus.

## Why

Four changes Cyrus asked for together, all about the sidebar's space and what it shows at a
glance:

1. "delete the outline section. I prefer an option that displays within the tab if I want that."
   The sidebar's Outline section takes a fixed slice of the sidebar and is empty ("No headings")
   whenever the focused tab is not markdown, which is most of the time.
2. The outline instead lives inside a markdown tab, after Material's guidance (researched
   2026-11-04): Material 3 Expressive's navigation rail, collapsed by default and expanded on
   demand, "standard" (beside the content) on a wide window and "modal" (over it, with a scrim) on a
   narrow one; drawn as a vertical stepper, the older Material pattern Cyrus remembered ("fixed-left
   vertical steppers ... responsive for narrow screens and minimize by default to preserve as much
   of the document's space as possible"). Cyrus chose, from the mockups
   (`docs/superpowers/brainstorm/7688-1793801419/content/outline-rail.html`), the expanded panel
   beside the document, and "keep the slim dot rail when closed".
3. "for the worktrees section, it should expand to occupy the remaining height when i open it or a
   minimum height; whichever is longer."
4. "I don't like the information that clicking the worktree gives. It's almost always 'clean'
   which is something I'd prefer to know by glancing at the list instead of clicking."

## 1. The sidebar Outline section goes

Removed: the section (its header, rows, chevrons, section preview beside the sidebar),
`outline.rs`'s sidebar rendering, `Sidebar::outline_open`, `outline_fade`, `outline_scroll`,
`outline_collapsed`, `Root::outline_hover`, and the saved `SidebarSaved::outline_open` (a saved
file that still carries it reads fine: the field is ignored). What `md_fold.rs` gives the outline
(`outline`, `visible_rows`, `current`, `preview`) stays: the rail below uses it. `docs/guide.md`'s
Outline paragraph is rewritten for the rail; `docs/development.md`'s Status row for the section
is replaced by the rail's.

## 2. The outline rail inside a markdown tab

### Where

In a viewer tab whose file is a markdown document (`Viewer::doc` is `Some`), in both the rendered
and the raw view (the headings come from the document either way). Not in presentation mode, which
shows the document alone. Not in any other kind of tab. In the rendered view the section in view
is `md_fold::current` of the first visible block; in the raw view it is the same function of the
block holding the editor's top visible line (`md_blocks::block_at`), and a click places the raw
view at the heading's line (my call).

### Collapsed: the slim dot rail (the default)

- A strip 26 px wide at the tab's left edge, inside the tab's surface, the document starting to
  its right. At its top a small menu glyph (☰) that expands it.
- A vertical stepper under the glyph: a thin line with a dot on it for each heading, a smaller dot
  for a heading one level below the document's top level; headings deeper than that have no dot
  (they appear in the expanded panel). Dots are spaced evenly, at least 16 px apart; when the dots
  do not fit the tab's height, only the top-level headings get dots, and if those still do not fit,
  the rail scrolls to keep the current dot in view.
- The dot of the section in view (`md_fold::current`) is filled in the theme's accent; the dots of
  sections above it are filled in the dim text colour (read); those below are hollow.
- The pointer on a dot shows the heading's text in a small tooltip beside it, after the app's usual
  tooltip delay; a click scrolls to the heading (`Viewer::reveal_block`, which unfolds it if a fold
  hides it).
- A tab with no headings shows no rail at all (no dots, no glyph): the document takes the full
  width.

### Expanded: the panel beside the document

- The ☰ glyph or the shortcut opens it: a panel 220 px wide in the rail's place, the document
  narrowing to its right (Material's "standard" rail). At its top a small "OUTLINE" label and a «
  that collapses it again.
- The same stepper with labels: one row per heading, indented by level, the line running down the
  left with its dots; the section in view has the filled accent dot, a tinted pill behind its row
  and its text in the normal text colour; sections above have filled dim dots; those below hollow
  ones. Long headings end in an ellipsis.
- A click on a row scrolls to its heading. The pointer resting on a row for half a second shows a
  preview of its section beside the panel (`md_fold::preview`, six lines, as the sidebar's
  Outline did).
- The panel scrolls when its rows do not fit.

### Narrow tabs: the panel over the document

- Below 520 px of tab width (my call), expanding lays the panel over the document from the left
  edge, with a scrim over the rest of the tab (Material's "modal" rail): the document keeps its
  width. A click on a heading scrolls to it and closes the panel; a click on the scrim or Escape
  closes it. The overlay takes the keyboard focus while it is open (Up and Down move between
  headings, Enter goes to one, Escape closes) and hands it back to the document when it closes
  (my call).
- A tab that narrows below 520 px with its panel open shows it as the overlay; widened again, as
  the side panel. The collapsed rail is the same at every width.

### Opening and remembering

- Per tab: the tab remembers whether its outline was expanded, across a relaunch
  (`Tab::Viewer` gains `outline: bool`, serde-default false, written only when true, as
  `presenting` is). A new markdown tab starts collapsed.
- A command "toggle outline" in the Viewer section of the keymap and the palette, default
  `alt-cmd-o` (the builder checks it is free; if not, picks a free one and says which), toggling the
  focused markdown tab's outline. Counted in telemetry as `toggle_outline` like the other palette
  commands (`telemetry/events.toml`, `docs/telemetry.md` regenerated).

### Look and motion

- Every colour is the theme's, read at paint: the line and hollow dots in `chrome.text_dim` at
  reduced alpha, read dots in `chrome.text_dim`, the current dot in the accent (`chrome.focus_ring`
  or what the tab's active underline uses), the tooltip and the preview in the menu frame
  (`chrome::menu_frame`). The marks are not text, so they clear 3:1 against the tab's surface in
  every built-in theme; the labels clear 4.5:1 (the contrast matrix test covers the new pairs).
- Expanding and collapsing follow the Motion rule: the width eases between 26 and 220 px over a
  Material duration token with the emphasized decelerate curve entering and accelerate leaving
  (a little quicker), the labels fading in after the width; the overlay slides in from the left
  with its scrim fading in. Reduce Motion makes them instant. Each adds itself to
  `Root::any_motion`.
- Under the effects (watercolor, space, matrix, notebook and the rest) the rail and panel sit on
  the tab's own surface as the document does; the hand checks below include them.

## 3. The Worktrees section fills the rest of the sidebar

- The sidebar holds only the Explorer and the Worktrees section once the Outline is gone; "the
  sidebar's height" is that column's height, both headers included.
- While the Worktrees section is open, the Explorer takes the height its rows need, and the
  Worktrees section takes everything below it, never less than a third of the sidebar's height
  (my call for the floor, so a long file tree cannot squeeze the list to a sliver); past that the
  Explorer scrolls instead. The Worktrees rows scroll inside their section.
- With Worktrees closed, the Explorer fills the sidebar as now. With the Explorer closed, the
  Worktrees section fills it from under the Explorer's header.
- Today's cap of 40% of the window's height on the Worktrees rows goes.

## 4. Worktree status at a glance

- Every listed side worktree's `git status` is known without a click: the row shows "N changes"
  (or "1 change") in the theme's modified colour when it has changes, and nothing extra when it is
  clean. "clean" is no longer written. N is exactly what the expanded list shows today
  (`gitstatus::run_status`'s entries, untracked files included, a rename one entry), from the same
  call, so the count and the list always agree.
- When it runs, kept light: once for each side worktree when the list first loads or a pass
  relists it; then for one worktree only when the watcher sees a change in it, in its files or in
  its admin folder (a commit changes its index there without touching a file), one second after the
  last such event (debounced per worktree). In `wt_watch`'s terms: a `Touched` worktree schedules a
  status run as well as its age; an event under its admin folder (`CheckStamp`) schedules a status
  run whether or not its stamp moved, since a commit rewrites the index there without touching
  `HEAD`, `gitdir` or `locked`; the stamp rule for relisting is unchanged. Never on a timer: the 10 second reload of the expanded
  worktrees' status goes. Arkos's own status runs pass `--no-optional-locks`, so they write nothing
  that would wake the watcher. On Cyrus's `~/Projects` that is ten status runs at load, then
  nothing while the worktrees sit still.
- A row with changes has its chevron and expands to list its changed files, each clickable to read
  as now. A clean row has no chevron and does not expand. A worktree that turns clean while
  expanded collapses.
- A missing worktree shows `missing` as now and runs no status.
- `arkos worktrees` is unchanged (it reports ages, not status).

## Docs and checks

- `docs/guide.md`: the Outline paragraph becomes the rail's (where it shows, the dots, expanding,
  the shortcut, narrow tabs, remembered per tab); the Worktrees paragraph says the section fills the
  sidebar and each row shows its changes at a glance.
- `docs/development.md` Status: replace the Outline row with the rail's checks (dots move with the
  scroll, a dot's tooltip and click, expand and collapse with motion, the overlay in a narrow pane,
  a relaunch keeps a tab's choice, under each effect); update the Worktrees row (the section fills
  the sidebar and keeps a third under a long tree; a change in a worktree shows "1 change" within a
  couple of seconds and a commit clears it).
- Tests, written first, named as sentences: the dot layout (spacing, top-level fallback, which
  dots exist), the dot states, the narrow threshold choosing overlay or side panel, the tab's saved
  `outline` field round-tripping and defaulting, the toggle command's binding, the Worktrees
  height rule as a pure function of the sidebar height and the Explorer's content height, the
  status debounce and which events schedule a status run, and the clean row having no chevron.

## Left to the builder

The module layout (an `outline_rail.rs` for the rail and panel is the obvious one); the exact
tooltip and preview placement; the dot sizes within the mockup's (12 px and 8 px); how the
Explorer reports its content height to the layout; the debounce's home (the driver from the
worktrees work, `sidebar.rs`, or a small pure helper beside it).

## Calls I made (Cyrus to check)

- The rail shows in raw view too, not only rendered; not in presentation mode.
- Dots for the top two heading levels only; deeper headings only in the expanded panel.
- A tab with no headings shows no rail at all.
- 520 px of tab width switches the expanded panel to the overlay; 220 px for the panel.
- `alt-cmd-o` for "toggle outline".
- The section preview on hover moves to the expanded panel; the sidebar Outline's per-heading
  collapse chevrons do not come back.
- Worktrees keeps at least a third of the sidebar; "clean" is no longer written; a clean row does
  not expand.
- In the raw view the current heading follows the editor's top line, and a click puts the editor
  at the heading.
- The narrow overlay takes the keyboard while open (arrows, Enter, Escape).
- The worktree count is the expanded list's own count; an admin-folder event refreshes status
  without relisting.
