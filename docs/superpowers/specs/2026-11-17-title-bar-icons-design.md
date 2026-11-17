# The title bar's four icons

Cyrus, 2026-11-17: change "Doctor" to "something resembling a hospital"; add "a 'Help' icon that
opens an in-app version of the Menu bar's 'Help' menu, a 'Settings' cog icon that opens the IDE's
settings", in the order "Notification, Doctor, Help, Settings", each with a tooltip; and "only
appear when the Icon button's menu is not already opened. I don't like it when an overlaid menu
has another menu overlaid on top of it."

- The doctor's icon is a hospital: a building on the ground, its roof stepped up in the middle, a
  cross on its front and a door at its foot (the wrench before it).
- Help is a question mark in a ring. Its popover hangs under it and lists the menu bar's Help
  items in their order (Keyboard Navigation, Keybindings, Documentation, What Arkos Sends); a
  click or Enter runs the item as the menu does and closes the popover.
- Settings is a cog; a click or Enter opens the Settings window, and no popover hangs.
- `chrome::BAR_ICONS` holds the order; a popover hangs under its own icon, counted from the bar's
  right end; the keyboard's ring walks the icons with Left and Right and stops at either end.
- An icon's tooltip shows only while its own menu is not open.
