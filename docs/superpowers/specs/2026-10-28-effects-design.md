# Effects: shaders under and over the window

Status: implemented 2026-10-28 (branch worktree-watercolor-spike); the spike it grew from is gone.
Drafted the same day from the spike (commits 041a5ed to 4ccddba) and builder-checked (rulings 9
to 12).

## 1. Intent

An effect is a shader that paints the window: a watercolor wash with soft bubbles under Peach
Light today, whatever a user or their coding agent writes tomorrow. Cyrus's words: a
differentiator, "watercolor effects on the IDE's UI components, subtle vertex-like movements
along the window boundaries, luminosity effects with subtle movements on the text". The spike
proved it is possible at a cost the MacBook Air can carry, and found the one hard boundary:
gpui draws the text, so painted text needs a patched gpui.

Effects are chosen in Settings like a font, not bound to a theme. A theme recommends one; the
user may pick another, or none, and keep it across themes. The effect is handed the theme's
colours and the shape of the UI, and derives everything else.

## 2. Goals, non-goals, anti-goals

Goals:
- Peach Light ships with the watercolor effect on by default, looking as the spike did at the
  end: washes that hug each block with a ragged, pooled rim; ink outlines with a slow wobble;
  bubbles drifting behind; the Explorer's selected and hovered rows and hovered tabs as fading
  strokes; glyphs written in ink; UI text two points larger.
- Any theme can carry any effect: the effect reads the theme's colours.
- A user, or an agent with the `arkos-ide` skill, can write an effect from the documented
  contract, check it compiles, measure what it costs, and install it, without reading Arkos's
  source.
- The GPU cost is known and bounded: measured by a command, reported in the window's log, and
  every technique's cost written down in the guide.
- Reduce Motion, a hidden or covered window, and the aux windows (About, Settings, Keybindings,
  the chooser) cost nothing: the first draws once and holds, the others never draw.
- Shader code lives in its own `.metal` files, never in Rust strings (Cyrus's preference: GitHub
  shows the language).

Non-goals (this milestone):
- Effects in the aux windows or the launch chooser.
- Per-element shader hooks beyond the glyphs (a shader on one pane, on the terminal's cells).
- Shaders in any language but Metal; Linux or Windows.
- Automatically disabling a slow effect. The cost is reported; the choice stays the user's.
- An effect editor in the app. The file is the editor.

Anti-goals:
- An effect that makes text harder to read than the theme's contrast rules allow. The paper
  under text keeps at least half its alpha (section 6.3), and the ink glyph shader never thins a
  stroke's core below 55% of its coverage.
- An effect that keeps the GPU busy while nothing moves. Idle under Reduce Motion means idle.
- A gpui patch that grows. The fork carries hooks, never features.

## 3. Constraints

- Every transition comes from `crates/arkos/src/motion.rs` and honours Reduce Motion (the
  Motion rule in CLAUDE.md). The spike added `HIGHLIGHT_ENTER` 250 ms on Material 3's standard
  decelerate curve and `HIGHLIGHT_EXIT` 200 ms on its standard accelerate; they stay.
- Every colour is read from `root.theme` at paint time: the effect reads the theme on every
  frame it draws, so a theme fade reads the blend.
- Builds on the Air: `nice -n 15`, `jobs = 3`, no `--release`, one cargo at a time.
- The window process's stderr is `/dev/null` when launched from the Dock: anything the effect
  has to say goes through the notice system or the log file, not `eprintln!` alone.
- A Unix socket cannot live on the external SSD; nothing here moves the daemon's folder.

## 4. The pieces

### 4.1 An effect is a folder

```
watercolor/
  effect.toml      # the manifest
  wash.metal       # the frame under the window (required)
  glyphs.metal     # the text's shader (optional)
```

Built-ins live in `crates/arkos/effects/<name>/` and are compiled into the binary with
`include_str!`. A user's live in `~/.config/arkos/effects/<name>/` (or under
`$XDG_CONFIG_HOME`); a folder there with a built-in's name replaces the built-in, as a theme
file does. `arkos effect list` shows both, marking a broken one `(invalid)` with its error.

The manifest:

```toml
name = "watercolor"
description = "A watercolor sketch: washes that hug each block, ink outlines, drifting bubbles."
wash = "wash.metal"
glyphs = "glyphs.metal"       # optional; absent, gpui draws text as always
frame_rate = 30               # 1 to 60; the wash's frames per second while anything moves
scale = 0.5                   # 0.1 to 1.0; the wash's pixels per window pixel, each side
ui_text_bump = 2              # 0 to 4 points added to the project window's UI text
ink_borders = true            # the app hides its own 1 px borders; the wash draws the edges
highlights = "effect"         # "effect": the wash paints the selected and hovered rows and the
                              # hovered tab from the highlight blocks; "app" (the default): the
                              # app keeps its own fills and reports no highlight blocks
paper_alpha = 0.5             # 0.5 to 1.0; a terminal's or document's paper over the wash
chrome_alpha = 0.0            # 0.0 to 1.0; the bars and the sidebar over the wash
active_tab_alpha = 0.55       # 0.0 to 1.0; the active tab over the wash
text_contrast = 0.0           # 0.0 to 1.0; how far the text moves towards ink (black on a
                              # light theme, white on a dark one) while the effect is on
```

Every key but `name` and `wash` has the default shown. Out-of-range values are refused with the
key and the range. `paper_alpha` floors at 0.5 for the anti-goal above.

### 4.2 The contract (what every shader is handed)

A preamble, `crates/arkos/effects/arkos.metal`, is prepended to every wash shader before it is
compiled, and is what `arkos effect contract` prints. It declares:

```metal
#include <metal_stdlib>
using namespace metal;

// The theme's colours, as float4 RGBA, by index.
constant int ARKOS_WINDOW_BG = 0;     constant int ARKOS_TITLE_BAR = 1;
constant int ARKOS_SIDEBAR = 2;       constant int ARKOS_TAB_BAR = 3;
constant int ARKOS_TAB_ACTIVE = 4;    constant int ARKOS_TAB_INACTIVE = 5;
constant int ARKOS_BORDER = 6;        constant int ARKOS_FOCUS_RING = 7;
constant int ARKOS_TEXT = 8;          constant int ARKOS_TEXT_DIM = 9;
constant int ARKOS_BANNER = 10;       constant int ARKOS_TERMINAL_BG = 11;
constant int ARKOS_TERMINAL_FG = 12;  constant int ARKOS_SELECTION = 13;
constant int ARKOS_WORKING = 14;      constant int ARKOS_WAITING = 15;
constant int ARKOS_BELL = 16;         constant int ARKOS_FIND_MATCH = 17;
constant int ARKOS_FIND_CURRENT = 18; constant int ARKOS_ANSI = 19; // 19..34: the sixteen
constant int ARKOS_COLORS = 35;

// What a block of the UI is.
constant int ARKOS_TITLE_BAR_BLOCK = 1;  constant int ARKOS_SIDEBAR_BLOCK = 2;
constant int ARKOS_DIVIDER_BLOCK = 3;    constant int ARKOS_TAB_BAR_BLOCK = 4;
constant int ARKOS_PANE_BODY_BLOCK = 5;  constant int ARKOS_ROW_HIGHLIGHT_BLOCK = 6;
constant int ARKOS_TAB_HIGHLIGHT_BLOCK = 7; constant int ARKOS_MEDIA_BLOCK = 8;
// A media block is a video's picture, opaque over the wash (added 2026-10-28 after 0.7.6).

// Which of a block's edges the app no longer draws (ink_borders): a bit each.
constant uint ARKOS_EDGE_TOP = 1;  constant uint ARKOS_EDGE_RIGHT = 2;
constant uint ARKOS_EDGE_BOTTOM = 4; constant uint ARKOS_EDGE_LEFT = 8;

struct ArkosBlock {
    float4 rect;      // x, y, width, height in window points, origin top-left
    float kind;       // ARKOS_*_BLOCK
    float edges;      // ARKOS_EDGE_* bits
    float seed;       // stable for the block's life: pick its grain from this, never from its index
    float value;      // a highlight's fade, 0 to 1 (1 for every other kind)
    float ordinal;    // a pane's place (0, 1, ...); a highlight's state (0 hovered, 1 selected, 2 both), else 0
    float3 pad;
};

struct ArkosInputs {
    float time;           // seconds since the effect started; frozen under Reduce Motion
    float intensity;      // the user's slider, 0 to 1 (config effect.intensity)
    float2 size;          // the window's content in points
    float scale;          // this drawable's pixels per point
    float block_count;
    float2 pad;
    float4 colors[35];
    ArkosBlock blocks[48];
};

struct ArkosVertex { float4 pos [[position]]; float2 uv; };
// The app supplies the vertex stage (a full-screen triangle). The shader supplies:
//   fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]);
```

The glyph shader's contract is gpui's sprite stage, through the fork's hook (section 4.5): a
`vertex` and a `fragment` function named `arkos_glyph_vertex` and `arkos_glyph_fragment`, with
the signatures of gpui's `monochrome_sprite_vertex` and `_fragment`, plus the two `[[flat]]`
fields the spike added (the glyph's atlas rectangle and its height). The preamble for glyph
shaders is the fork's own sprite declarations; `arkos effect contract --glyphs` prints them.

What is not in the contract, on purpose: the panes' tabs one by one, the text's positions, the
Explorer's rows. Blocks are what the shader may paint; highlights are the only elements that
report themselves, because they fade.

### 4.3 Choosing an effect

`config.toml`:

```toml
[effect]
name = "theme"      # "theme": the theme's choice; "none"; or an effect's name
intensity = 1.0     # 0.0 to 1.0, passed to the shader
```

A theme's `[effect]` section has one key, `name`, the effect it was designed with; a theme
without one recommends none. Peach Light recommends `watercolor`.

Settings, Appearance section, under the themes: **Effect**, a list like the theme list ("Theme's
choice" first, then "None", then every effect, a configured one that is missing listed as "(not
installed)"), an **Intensity** stepper (0.0 to 1.0 by 0.1), and "Show effects folder". The
"Theme's choice" is always named with the current theme's recommendation, as in "Theme's choice
(watercolor)", and reads "Theme's choice (none)" when the theme recommends nothing.

`arkos settings set effect.name <theme|none|name>` and `effect.intensity <0..1>` join the keys.

Switching takes effect live, and as a transition under the Motion rule: the cross-fade lives in
the surfaces' alphas, which move between opaque and the effect's style over
`motion::THEME_FADE` (200 ms, in step with a theme fade when both change at once) while the
wash's layer stays at full opacity under them, the effect counting in `Root::any_motion`
meanwhile; the glyph shader and the UI text size, which can only cut, switch at the fade's end,
so the layout settles once. At the same moments the window's theme fades to one derived from the
configured theme by the manifest's `text_contrast` (`chrome.text`, `chrome.text_dim` and
`terminal.foreground` that far, three quarters and nine tenths of the way towards ink), and
back. Under Reduce Motion the whole switch cuts. The window's background turns transparent at
the fade's start when an effect comes and opaque at its end when one goes, so nothing shows
through meanwhile. A theme switch with `name = "theme"` does the same when the recommendation
changes. The spike's "relaunch to turn it off" is gone.

### 4.4 How the wash is drawn

As the spike: a `CAMetalLayer` in its own `NSView` below gpui's view in the window's content
view; the window's background transparent while an effect is on, opaque otherwise (gpui's
`set_background_appearance`, live); the app's surfaces painted with the manifest's alphas. The
layer draws from its own timer at `frame_rate` while anything moves, sixty while a highlight
fades, and not at all when nothing has changed under Reduce Motion, when the window is hidden,
minimized or occluded (its `occlusionState` without `Visible`: wholly behind other windows or on
another Space), or when it is covered (`Root::covered`, as the web views use). gpui redraws
nothing for the wash. Not drawing means no work either: while the window cannot be seen the
timer builds nothing and looks again every half second; under Reduce Motion it stops once
nothing moves, and `Root::render` (which runs on every change the wash reads: the layout, the
highlights, the theme's blend, the intensity, the size) starts it for one tick after the frame,
running on only while a stroke or a fade moves.

The blocks: the title bar; the sidebar and its divider between Explorer and Worktrees; each
pane's tab bar and body from `Root::layout()`; the Explorer's selected and hovered rows and the
hovered tab from the highlight registry (4.6). `effect::blocks(&Root) -> Vec<Block>` is pure over
the layout and the registry, and tested: which blocks for which layout, which edges, the stable
seeds (a pane's id, a highlight's key), the ordinals.

Every sixtieth frame the command buffer's GPU time is read and kept as a running average; the
window log says `effect watercolor: {ms} ms per frame at {w}x{h}` once a minute while it changes
by more than a tenth, and `arkos effect bench` reads the same number (4.7).

### 4.5 The gpui fork and the glyph hook

gpui 0.2.2 moves to `https://github.com/csarkosh/gpui`, a repository seeded from the crates.io
sources of 0.2.2 (what `vendor/gpui` held while it was built, 8 MB, not the Zed monorepo),
with Arkos's patches as commits on top and tags `v0.2.2-arkos.N`. Arkos depends on it through
`[patch.crates-io] gpui = { git = ..., tag = ... }`; `vendor/gpui` leaves the tree. An upgrade
imports the next crates.io sources as a commit and rebases the patches. The repository is
public (the sources are Apache-2.0 and the release workflow clones it without credentials).
My call, flagged: a new repository rather than a fork of `zed-industries/zed`, because the
monorepo is a gigabyte that every clone and every CI run would pay for.

The first patch is one hook, no feature, on the window, so the project window takes it and
the About, Settings, Keybindings and chooser windows keep gpui's own glyphs:

```rust
/// Replaces this window's monochrome sprite (glyph) stages with the named functions from
/// `source`, which is appended to gpui's own shader source before the window's Metal library
/// is built. `None` restores gpui's own. The window rebuilds that one pipeline at its next
/// frame. `Err` carries the Metal compiler's message when the source does not compile, and
/// gpui's own stages stay in place.
impl Window {
    pub fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String>;
}
pub struct GlyphShader { pub source: String, pub vertex: String, pub fragment: String }
```

A rebuild compiles the library once: tens of milliseconds, once per switch. The app turns an
`Err` into a notice and draws the theme plain.

The plan builds and tests the hook against an in-tree copy of gpui first (`[patch.crates-io]
gpui = { path = "vendor/gpui" }`, as the spike does), and moves to the git dependency as its
last task, once the repository exists; the release workflow switches over then.

The ink glyph shader itself (the spike's `monochrome_sprite_fragment_ink`, with the widened
quad and the size-aware variation) moves into `crates/arkos/effects/watercolor/glyphs.metal`.
The fork carries no Arkos code.

### 4.6 Highlights

The spike's thread-local marks become fields of `Root`: `effect_marks: Vec<Mark>` cleared at
the start of each render and filled by the elements that fade (Explorer rows through their
canvas, tabs through theirs), and `effect_hover: Option<u64>` set by their `on_hover`. The
strokes' fades (`effect::Strokes`, the spike's `strokes()`) live in `Root` too, keyed by the
mark's key, with the motion module's `highlight_in` and `highlight_out`. Under Reduce Motion a
stroke is at its target at once, and the wash draws once for the change.

Strengths are the shader's business: a highlight block carries `kind`, `value` (its fade) and
whether it is selected, hovered or both through `ordinal` (0 hovered, 1 selected, 2 both), and
the watercolor shader picks 0.2, 0.3 and 0.36 as the spike did. Only an effect whose manifest
says `highlights = "effect"` gets highlight blocks and takes the app's own fills away; with
`"app"` the app paints its selected row, hovered row and hovered tab as it does without an
effect (gpui's hover style, instant), and the registry stays empty. `arkos effect check` says
which the manifest chose, so an author who forgot to paint highlights sees why nothing shows.

### 4.7 `arkos effect`, for agents and people

- `arkos effect list [--json]`: every effect with its source (built-in, folder, invalid and why).
- `arkos effect contract [--glyphs]`: prints the preamble (4.2), the whole contract.
- `arkos effect template --as <new-name> [<built-in>]`: writes a copy of a built-in (default
  `watercolor`) into the effects folder under the new name, with the manifest's `name` set to
  it, and prints the folder's path. The starting point for an agent. An existing folder of that
  name is refused.
- `arkos effect check <folder> [--json]`: parses the manifest and compiles both shaders against
  the contract with Metal; prints `ok: <name>` or the compiler's errors with line numbers
  (offset by the preamble, so they point into the user's file). Exit 1 on an error.
- `arkos effect bench <folder|name> [--seconds N] [--json]`: runs the wash off-screen at the
  size of the first window to answer (with several open, not necessarily the front one) for N
  seconds (default 3) and prints the GPU time per frame, the share of
  a frame at the manifest's rate, and a verdict against the budget (4.8). Needs a window for the
  size and the theme; without one, 1200 by 800 and Peach Light.
- `arkos effect use <name|none|theme> [--json]`: sets `effect.name` and reloads, as `theme use`.
- `arkos effect current [--json]`: the config's choice and what each window draws, with its cost.

All of them are plain commands, testable with the harness `theme` uses; `check` and `bench`
need Metal, so their tests run only where `Device::system_default()` answers and are `#[ignore]`
elsewhere.

### 4.8 The budget, and what the guide says

The guide gets a section, "Effects", in the voice of the rest of it: what an effect can touch
(the wash under everything, the text through the glyph shader, the alphas of the app's
surfaces, the borders, UI text size) and cannot (anything else gpui draws, the aux windows, the
layout); the contract by reference to `arkos effect contract`; the steps (template, edit,
check, bench, use); and the costs, measured on the Air at 1200 by 800 points, Retina:

| Technique | GPU per frame | Of a 30 fps frame |
| --- | --- | --- |
| Per-pixel noise evaluated once per block (22 blocks, 3-octave fbm each), quarter scale | 8.5 ms | 25% |
| The same with the noise fields shared by every block, quarter scale | 0.64 ms | 2% |
| Shared fields plus ink outlines, half scale (the outlines need it) | 3.6 ms | 11% |
| Ink glyphs on top (gpui's own pass; only when text changes) | about 0.1 ms per repaint | |

The budget: stay under 4 ms per frame at half scale; `bench` says `within budget`, `heavy` from
4 to 8, `too heavy` above 8. Rules of thumb the guide states: compute a noise field once per
pixel and read it per block, never per block per pixel; draw at the lowest `scale` the look
allows (the ink needs 0.5, a soft wash 0.25); keep `frame_rate` at 30 unless something fast
moves; place anything that moves on the CPU side of the contract when it is per block, and in
the shader from `time` when it is per pixel; and pick every block's grain from `seed`, never
from its index, or a highlight coming and going will reshape the panes.

### 4.9 Docs and status

- `docs/guide.md`: Settings Appearance gains Effect and Intensity; a short "Effects" paragraph
  for people (what it is, where the folder is, that an agent can write one).
- `docs/themes.md`: the `[effect]` section.
- `docs/agent-guide.md`: the Effects section (4.8) and the `arkos effect` commands (4.7); the
  skill's stamp bumps so installed skills refresh.
- `docs/development.md`: the effect's place in the Motion rule (its timer, the frozen time);
  status rows: Peach Light shows the wash, the divider, the fading strokes on rows and tabs, the
  ink glyphs; switching to None removes it live; Navy with watercolor reads navy; Reduce Motion
  freezes it and Activity Monitor shows the window idle; a covered window draws none; a shader
  that fails to compile shows a notice and the theme draws plain.
- This spec's status line.

## 5. What is left to the builder

Names of the Rust modules (the spike's `effect.rs` splits at least into the manifest and
loading, the blocks, the strokes, the layer); the exact Settings layout within the Appearance
section's existing style; the notice wording; the preamble's helper functions, if any (the
spike's noise helpers belong to the watercolor shader, not the contract); how `bench` runs
off-screen (a layer that never presents, a texture target); the tarball-free form of
`template`; the fork's first commit message and the tag name.

## 6. Rulings (check these while reading)

1. Effects are chosen in config and Settings, not by the theme; the theme recommends (Cyrus).
2. The shader gets the theme's colours and the blocks; it derives every shade (Cyrus).
3. The gpui fork is a new repository seeded from the crates.io sources, public, with one hook
   patch; the ink shader stays in Arkos (my call, 4.5).
4. Bubbles are not in the contract: the watercolor shader places them from `time`; the spike's
   CPU placement was a performance fix for a shader that no longer needs it (my call).
5. A highlight's strength is the shader's choice from its kind and state, not the app's (my
   call, so a user's effect can highlight differently).
6. `paper_alpha` floors at 0.5 and the ink glyph core at 55%: readability is not the shader's
   to give away (my call, from the anti-goal).
7. No automatic disabling of a slow effect; the cost is reported and the choice is the user's
   (my call; revisit if a user ships a 25% shader).
8. Highlights fade over Material 3's standard timing (Cyrus): 250 ms in, 200 ms out.
9. (builder check) The glyph hook is per window, so the aux windows and the chooser keep
   gpui's own glyphs; the fork's API is `Window::set_glyph_shader` (my call).
10. (builder check) The hook is built against the in-tree copy first and the move to
    `csarkosh/gpui` is the plan's last task; Cyrus creates the repository, or says to (my call
    on the order; the repository is his).
11. (builder check) A manifest chooses `highlights = "effect"` or `"app"`, default `"app"`, so a
    user's effect that paints no highlights loses nothing; watercolor says `"effect"` (my call).
12. (builder check) An effect coming or going cross-fades over `THEME_FADE` with the glyphs and
    the text size switching at the end; Reduce Motion cuts (my call, from the Motion rule).
