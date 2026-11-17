# The title bar's four icons

Cyrus, 2026-11-17: change "Doctor" to "something resembling a hospital"; add "a 'Help' icon that
opens an in-app version of the Menu bar's 'Help' menu, a 'Settings' cog icon that opens the IDE's
settings", in the order "Notification, Doctor, Help, Settings", each with a tooltip; and "only
appear when the Icon button's menu is not already opened. I don't like it when an overlaid menu
has another menu overlaid on top of it."

- The doctor's icon is a first-aid kit: a case with rounded corners, its handle on top and a
  cross on its front (the wrench before it; a hospital building first, which did not read:
  "something indicating 'Medical' so the user gets an impression that it fixes issues with the
  IDE").
- Help is a question mark in a ring. Its popover hangs under it and lists the menu bar's Help
  items in their order (Keyboard Navigation, Keybindings, Documentation, What Arkos Sends); a
  click or Enter runs the item as the menu does and closes the popover.
- Settings is a cog; a click or Enter opens the Settings window, and no popover hangs.
- `chrome::BAR_ICONS` holds the order; a popover hangs under its own icon, counted from the bar's
  right end; the keyboard's ring walks the icons with Left and Right and stops at either end.
- An icon's tooltip shows only while its own menu is not open.

## Tabs from the keys

Cyrus, 2026-11-17: alternatives to Ctrl+Tab, "re-using existing navigation rules"; he chose the
pane's number again. A pane's Cmd+number pressed while that pane already has the keys rings its
tab bar (`Root::tab_ring`): Left and Right show the tab before or after, stopping at either end,
as the switcher and the title bar's icons do; Enter, Down or Escape leave the ring with the keys
in the tab shown; any other key, a press anywhere or another command takes the ring away. The
ring is the keyboard's ring in the effect's own shape (`chrome::keys_ring_inset`). Ctrl+Tab and
Ctrl+Shift+Tab are no longer bound; their actions stay in the palette, and Option+Cmd+Left and
Right still step the tabs.

## The menus from the keys

Cyrus, 2026-11-17: the menus the Settings and Help icons open "must support keyboard navigation
too".

- A popover opened with a click takes the keys (`Root::click_popover`): nothing is marked, Down
  marks the first row and Up the last, Enter presses the one marked. No ring shows on the icon,
  since the pointer put it there, and when the popover closes the keys go back where they were
  (`Root::bar_by_mouse`). From the keyboard nothing changes: the ring stays on the icon.
- The Settings window goes by the IDE's rules (`settings::settings_key`): the first arrow shows
  the ring where the keys are; in the sections' column Up and Down show the section before or
  after, Right, Enter or Space go into its controls, Escape closes the window; in the controls Up
  and Down move through every control that does something (in the order drawn, the ones in force
  skipped), Enter or Space press it, Left or Escape go back to the column. Each control registers
  itself with what a press does as it is drawn (`SettingsWindow::act`), so a click and Enter run
  the same code. The ring is drawn two points outside the control, following its corners, so it
  never reads as a selected pill's own border; the ringed control scrolls itself into view. A
  pointer press hides the ring until the next key.
- The screenshot shoot floats its window; a window opened over it (Settings, Keybindings) floats
  too, or it would open hidden behind, and the shoot can press Cmd+, (the comma's key code).
