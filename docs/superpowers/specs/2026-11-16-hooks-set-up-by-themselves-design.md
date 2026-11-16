# Claude Code's hooks set up by themselves

Cyrus, 2026-11-16, of the marketing page's install line: "I also don't like that we have to ask
them to run the "Install Claude Code hooks" from the command palette. Is this something that can
be done automatically? I prefer a UX where they install it and everything just works from them."

## What changes

The window's startup step for Claude Code's config folders (`Root::follow_theme_in`, which
already brings Claude Code's Arkos theme up to date in each folder Arkos knows, and runs again
for a folder first seen in use) also sets up the hooks: each folder's `settings.json` whose
hooks are missing, or name another binary (an older or moved app), gets them merged in as
"Install Claude Code hooks" does, a `.bak` written first (`hook_cli::install_missing`).

- A file whose hooks are current is not touched, so a normal start writes nothing.
- A file that does not parse is left as it is and logged; the doctor's title hook row already
  names it.
- When anything was written the bell says so, naming each file:
  "Arkos set up Claude Code's hooks in ~/.claude/settings.json, so each tab shows what its
  session is doing; restart claude in a tab already running it" (notice kind `claude_hooks`,
  added to the telemetry allowlist).
- Only the app run from its bundle does it (`hook_cli::installs_by_itself`): never a sandbox,
  and never a build from source, whose hooks would point every Claude Code at `target/debug`
  and fight the installed app over the file.

"Install Claude Code hooks", `arkos hooks install` and the doctor's row stay, for a repair or a
Claude Code folder used outside Arkos.

The README's install paragraph (the apps.csarko.sh page's) drops the signing and the palette
step: "Open it and start `claude` in a tab: the tabs follow what each session is doing from the
first launch, and from then on the app updates itself."

## Tested

Headless: `installs_by_itself` (the bundle, a sandbox, a build from source); `install_missing`
over a folder with no file, one current, one from a moved app and one that does not parse; the
notice's text. By hand: the Status row "hooks set up by themselves".
