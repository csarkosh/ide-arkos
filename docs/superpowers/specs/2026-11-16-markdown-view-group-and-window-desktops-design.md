# The markdown view group, image zoom, the rename preview and window desktops

Cyrus, 2026-11-16: the markdown tab's `⋯` button becomes "a compact icon button group for
`Present | Edit | Raw`", each with a tooltip saying what it does; a double click on an image in
Present and Edit toggles a zoomed view of it; when the IDE updates itself, every window comes
back on its own virtual desktop ("they all re-open in 1 desktop and I need to move them back");
and while a session is renamed, its row in the Sessions list shows the name as it is typed.

## 1. The view group

Three icon buttons in one translucent pill at the top right of a markdown tab, where `⋯` stood:
Present (a screen with a play mark), Edit (a pencil; Read for a file that cannot be written)
and Raw (code brackets). The view in force is lit in the accent; another is tinted under the
pointer and a click switches to it through the toggle its key runs (`view_menu::step`:
Option+Cmd+P for presenting, Shift+Cmd+M for raw), so saving and telemetry are the keys'. A
button held under the pointer shows gpui's tooltip: the name, the keys, and a line on what it
does. The menu's other rows go elsewhere: the inline diff is the chip other files already have,
left of the group; the outline is the ☰ on its rail and Option+Cmd+O.

## 2. Image zoom

A double click on an image in the rendered view (presenting or editing; the raw view has no
pictures) emits `ViewerEvent::ZoomImage`, and the window shows the image as large as the pane
holds it, contained, over the document on its ground at 96%, fading as the menus do
(`Root::image_zoom`, a `Transient`). A double click on it, Escape or the tab leaving the front
puts it back; the same double click on another image swaps it in (`view_menu::zoom_after`).

## 3. The rename preview

A session's row is named for its original terminal; while that tab is being renamed, the row
shows what is typed (`sessions_list::shown_name`), a blank name the automatic one.

## 4. Window desktops

macOS has no public call for a window's Space. SkyLight's private calls answer a process about
its own windows (a spike on macOS 26.6, 2026-11-16: `SLSCopySpacesForWindows` reads it,
`SLSMoveWindowsToManagedSpace` moves a new window to a desktop not in front, which stays in front;
`SLSCopyManagedDisplaySpaces` lists every desktop with a uuid that lasts across relaunches; a
display's first desktop has an empty uuid). `spaces.rs` looks them up at run time.

- Saved: `WindowSaved.space`, the uuid of the desktop the window is on, noted as the window is
  activated or left and as it quits (the update's relaunch quits through `flush_for_quit`).
- Restored: only for a window the daemon relaunched (`ARKOS_RELAUNCH`, which also keeps it from
  taking focus), right after the window is made; a desktop that has gone leaves it where macOS
  put it. A window the user opens opens on the desktop they are on.

Checked by hand in the dev sandbox: a window saved on desktop 3 and started as the daemon starts
it came up on desktop 3 while desktop 2 stayed in front.
