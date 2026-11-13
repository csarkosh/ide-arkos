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
- **Custom.** A colour and an effect, chosen apart: any theme's colours under any effect, or
  none. The fonts go with the effect when it names its own (see "Fonts" below). There is one custom look: it does not follow macOS's light and dark.
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
color = "navy"         # a theme's name: its colours (and its fonts, under an effect with none)
effect = "none"        # "none" or an effect's name
```

- `appearance.look` and `[custom]` are new. `custom.color` unset means the colours are the
  preset the mode picks; `custom.effect` may also be `theme`, the colour's own.
- `effect.name` stays readable. A config with no `look` says which look by it: `theme` is the
  presets; `none` or a name was that effect over whichever theme showed, which is a custom look
  with no colour of its own (so it still follows macOS, as it did). Nothing is rewritten at
  launch. Once a look is written, `effect.name` says nothing more. `effect.intensity` and
  `effect.run_unfocused` stay where they are.

## Settings, Appearance

Two pills at the top, **Presets** and **Custom**. The one in force is selected; a click on the
other puts it in force. Under them, only the selected one's controls:

- **Presets**: Mode (System, Dark, Light); "Dark preset" and "Light preset", the two lists as
  today; "Add preset…" (a theme file, as "Add theme…" was) and "Show presets folder". (A row
  does not name its preset's effect yet: the list has only names and kinds.)
- **Custom**: "Color", one list of every theme, dark ones first, each marked dark or light;
  "Effect", "None" and then every effect installed, built in or a folder, an invalid one marked
  and not clickable; "Add color…" (a theme file, selected as the colour), "Add effect…" (an
  effect's folder, copied into the effects folder and selected) and the two "Show … folder"
  buttons.

The first click on Custom, with no `[custom]` table yet, starts from what is on screen: the
preset showing as the colour and its effect as the effect, so nothing changes until the user
picks something.

## Fonts

Cyrus, 13 November, on the first build: "why doesn't the effect's font remain when I change
colors for Custom". Fonts were a theme's alone, so the pixel face left with Timo Purple's
colours while overworld stayed. Now an effect's manifest may carry a `[font]` table, the
theme's own keys, and `theme::fonts_for` picks:

- Custom: the effect's fonts when it names any, whatever the colour; else the colour's.
- Presets: the theme's own when it names any (a preset is a finished look); else its effect's.

`overworld` names Departure Mono on its 5.5 grid and `notebook` Chalkboard SE and Monaspace
Radon. `watercolor` and `material` name none: each has two presets with different faces, and
naming one would change a font on the first click on Custom, so under them the font is still
the colour's. (My call.) The theme files keep their `[font]` tables, so nothing changes for a
preset or for a config from before the two looks.

## The commands

The agent guide's commands keep working, in the new terms:

- `arkos theme use <name> [--dark|--light]` sets that preset. When the custom look has a colour
  of its own, which would go on showing, it also puts the presets in force; a custom look with
  only an effect keeps it over the theme named, so someone who turned effects off keeps them off.
- `arkos effect use <name|none>` puts custom in force with that effect, over its own colour or,
  when it has none, the theme showing. `arkos effect use theme` puts presets in force.
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
