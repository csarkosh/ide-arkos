# Arkos IDE: reading niceties (milestone A8)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, sections 3.8, 3.9 and 9); Cyrus asked to start A8 on
2026-10-30. Builds on A1 to A7 (merged at `e89002b`).

## 1. Alerts

A quote whose first line is `> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]`
(any case) is an alert: its bar and a header line in the kind's colour, the header an icon and the
kind's name, or the text after the tag when there is some (`> [!TIP] Shortcut`). The tag line is
not drawn as text. With the caret in it the header stays and the text reveals as any quote's does,
the tag line with it. Colours come from the theme at paint time: note the link colour, tip the
diff's added green, important a violet, warning the theme's warning yellow, caution its error red.
The icons are small SVGs of Arkos's own (an info circle, a bulb, a speech bubble, a triangle, an
octagon), tinted with the colour, served by an asset source the app now has.

## 2. Footnotes

GFM footnotes are parsed (`[^key]` references, `[^key]: text` definitions).

- **References** are numbered by their first appearance in the document, 1, 2, 3 whatever the
  keys, and drawn as a small raised number in the link colour. Hovering one shows the note's text
  in a popover beside it. Cmd+click (a single click in presentation mode) scrolls to the note. A
  click reveals `[^key]` like any other element. A reference to a key with no note is drawn as
  written.
- **Definitions** draw where they are, in a smaller dim size: the number, then the text, then a
  `↩` whose click scrolls back to the first reference.
- `/footnote` (A3) already inserts a reference and its definition.

## 3. Front matter

A front matter block draws as a faint, small label (`front matter`), with above it the `title` as a
title (the size of a level 1 heading) and the `date` or `created` as a dim line when the front
matter has them. A click shows its source, as today; leaving draws the label again.

## 4. Presentation mode

A read-only toggle per tab. A second pill left of the mode pill reads `present`; a click (or
Shift+Cmd+P's palette entry "Toggle presentation mode", bound to Alt+Cmd+P) turns it on, and the
pill then reads `edit`. While on: no caret and no reveal, nothing edits (keys, paste, drop, menus
that change text), a single click follows a link or a footnote, the drag handles, the table's
strips and edge selectors, the code block's language list and the right-click formats are gone,
and front matter shows only its title and date. Raw mode turns it off. Not saved with the tab.

## 5. Page width and font size

`[viewer] page_width = "full"` (the default) or `"letter"`: letter keeps the rendered text in a
column at most 816 px wide (a letter page at 96 dpi), centred in a wider pane. The Settings window
gets a "Page width" row beside "Wide tables", and `arkos settings set viewer.page_width letter`
sets it. The font size steps Mark Sharp has are Arkos's Cmd+= and Cmd+- in the focused viewer,
already there; the guide says so.

## 6. Tests and the screen pass

Headless: alert detection and its title, runs without the tag line; footnote numbering by first
reference, definitions and their text; front matter title and date; presentation's gate on
edits; the page width's column. On the local shoot: the five alerts, a footnote's number, hover
and jump and back, the front matter label and title, presentation on and off with a click on a
link, letter width in a wide window, Cmd+= in a document.
