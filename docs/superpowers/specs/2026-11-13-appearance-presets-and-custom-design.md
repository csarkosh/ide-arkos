# Appearance: presets or custom

## Intent

Cyrus, 13 November: "i want to rethink the mental model of the appearance. it should be changed
to a 2 options system: presets, custom. `Presets` include the Mode, Dark Theme, and Light Theme
options. if selected, these configurations take effect. They can add their own presets. `Custom`
includes "Color" and "Effects" so the user can mix and match what they want. they can add their
own colors and effects too. Also, rename "runic-navy" to "tokyo-rain"."

Today Settings shows a Mode, a dark and a light theme, and under them an Effect list whose
"Theme's choice" row is the only hint that a theme brings an effect with it. Two ideas are mixed
in one column: a theme as a finished look, and a theme as a palette to put an effect on. This
design splits them.

## The model

The look of the window is one of two things, and only one is in force:

- **Presets.** A preset is a finished look: colours, the effect made for them and its fonts.
  It is what a theme file has always been. The user picks a Mode (System, Dark, Light), a dark
  preset and a light preset, and each preset draws with its own effect.
- **Custom.** A colour and an effect, chosen apart: any theme's colours (and fonts) under any
  effect, or none. There is one custom look: it does not follow macOS's light and dark.
  (My call from "Custom includes Color and Effects", which names no Mode.)

Switching between the two never loses the other's choices.

## The config

```toml
[appearance]
mode = "system"        # presets: which of the two shows
look = "presets"       # "presets" or "custom"

[theme]                # the presets
dark = "navy"
light = "peach-watercolor"

[custom]               # the custom look
color = "navy"         # a theme's name: its colours and fonts
effect = "none"        # "none" or an effect's name
```

- `appearance.look` is new and defaults to `presets`. `[custom]` is new.
- `effect.name` goes. Under presets the effect is the preset's own; under custom it is
  `custom.effect`. A config that still has `effect.name` loads: `theme` means nothing more, and
  `none` or a name is read as the old meaning (that effect over the picked theme) until the
  window rewrites the file at launch, as it does a renamed theme's name: `look = "custom"`,
  `custom.color` the theme that was showing, `custom.effect` the old value, and `effect.name`
  removed. `effect.intensity` and `effect.run_unfocused` stay where they are.

## Settings, Appearance

Two pills at the top, **Presets** and **Custom**. The one in force is selected; a click on the
other puts it in force. Under them, only the selected one's controls:

- **Presets**: Mode (System, Dark, Light); "Dark preset" and "Light preset", the two lists as
  today, each row naming the preset's effect after its name; "Add preset…" (a theme file, as
  "Add theme…" was) and "Show presets folder".
- **Custom**: "Color", one list of every theme, dark ones first, each marked dark or light;
  "Effect", "None" and then every effect installed, built in or a folder, an invalid one marked
  and not clickable; "Add color…" (a theme file, selected as the colour), "Add effect…" (an
  effect's folder, copied into the effects folder and selected) and the two "Show … folder"
  buttons.

The first click on Custom, with no `[custom]` table yet, starts from what is on screen: the
preset showing as the colour and its effect as the effect, so nothing changes until the user
picks something.

## The commands

The agent guide's commands keep working, in the new terms:

- `arkos theme use <name> [--dark|--light]` sets that preset and puts presets in force.
- `arkos effect use <name|none>` puts custom in force with that effect, over the colours on
  screen when custom had none of its own. `arkos effect use theme` puts presets in force.
- `arkos settings set` takes `appearance.look`, `custom.color` and `custom.effect`; it still
  takes `effect.name`, as `effect use` does.
- `arkos theme current` and `arkos effect current` say which is in force.

## The rename

The built-in theme `navy-runic` becomes `tokyo-rain`. The old name still loads it, and a config
naming it is moved to the new name at launch (`theme::RENAMED`).

## Constraints

- Colours are still read from `root.theme` at paint; nothing here caches one.
- A theme switch through either look fades as today.
- Tests first, headless, named as sentences. `cargo check --workspace --all-targets` with no
  warnings. `docs/guide.md`, `docs/agent-guide.md` and `docs/themes.md` are kept true, and
  `docs/development.md` gains the Status rows only a person at the screen can check.
