# The `overworld` Effect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A built-in `overworld` effect, recommended by Timo Purple. It draws a muted pixel-art night overworld in the window's chrome. The tab bodies stay plain.

**Architecture:** There are two small app changes, then the effect, then the theme and docs.
- **The foot block:** the sidebar reports where its names end. This becomes a new `SidebarFoot` block the wash can fill.
- **The body bars key:** a new manifest key, `body_bars_alpha`, keeps four bars inside tab bodies opaque while the chrome goes clear.
- **The effect:** a folder of `effect.toml` and `wash.metal`. Its timing and layout arithmetic are mirrored in a test-only Rust module, as `space.rs` and `runic.rs` mirror theirs.

**Tech Stack:** Rust, gpui (csarkosh fork), Metal Shading Language, TOML.

**Spec:** `docs/superpowers/specs/2026-11-04-overworld-effect-design.md`. The mockups are in `docs/superpowers/brainstorm/52431-1793848018/content/`: `overworld-v2.html` (A2) and `overlap.html` (option 1).

## Global Constraints

**Build and check**
- Builds use `nice -n 15 cargo`, never `--release`, one cargo at a time.
- The worktree keeps the shared `target-dir` in `.cargo/config.toml`. Copy it from the main checkout; it is excluded from git.
- Tests are headless, written before the code they check, and named as sentences.
- Finish every task with `nice -n 15 cargo check --workspace --all-targets` showing no warnings. The pre-existing "future version of Rust" note is fine.
- Workspace tests: `nice -n 15 cargo test --workspace -- --skip a_shell_answers_to_the_daemon_s_responsible_process --skip concurrent_spawns_give_each_shell_only_its_own_terminal`.

**Writing**
- Doc comments are plain prose, with no em-dashes.

**Shaders and colour**
- Every shader lives in its own `.metal` file loaded with `include_str!`, never in a Rust string.
- Every colour comes from the theme each frame (`u.colors[...]`). Nothing is baked in except mixing amounts.
- Never loosen a contrast floor to pass.

**The art (from the spec)**
- An art pixel is 3 points square, snapped to whole window pixels.
- The scene is at most about 140 points tall, and is gone below 50 points of room.
- Art colours are mixed 45% of the way toward the sidebar colour. Only the castle windows, the lantern's flame and the fireflies keep their full colour.
- Under Reduce Motion the clock is exactly 0, so time 0 is the frame with everything lit.

**Cost**
- `arkos effect bench overworld` must say `within budget` (under 4 ms per frame).

**Copy**
- The description is exactly: `A night overworld in muted pixel art at the foot of the sidebar: stars, a castle, pines and fireflies.`

## Review Focus

1. **A long Worktrees list that scrolls.** The last row is below the sidebar's bottom, so there must be no foot block and no scene, never a negative-height one. Pinned by `no_foot_is_reported_when_the_names_reach_the_bottom` (Task 1).
2. **Sliding the sidebar shut (Cmd+B).** The foot must follow the sidebar's shown width, never the settled one, and vanish with the sidebar. Pinned by `the_foot_is_as_wide_as_the_sidebar_on_screen_and_gone_without_one` (Task 1).
3. **A full block list.** At the 48-block cap the foot must never be the block dropped, or the scene vanishes in a busy window. Pinned by `the_foot_comes_before_the_panes_so_the_cap_never_drops_it` (Task 1).
4. **`highlights = "effect"`.** The app stops filling the selected and hovered rows and the hovered tab, so if the wash forgets them, the selection disappears. Pinned by the Status row in Task 4 and the reviewer's read of the wash's highlight branch (Task 3).
5. **Banners in a grid and in a spreadsheet's strip.** They must keep their own colour under overworld, and change for no existing effect. Pinned by `the_body_bars_follow_the_chrome_unless_the_manifest_says_otherwise` (Task 2).

---

### Task 1: The sidebar reports where its names end

**Files:**
- Modify: `crates/arkos/src/effects/blocks.rs`: add `Kind::SidebarFoot = 16`, `Scene::sidebar_end`, and the foot block in `blocks()`.
- Modify: `crates/arkos/effects/arkos.metal:27`: add `constant int ARKOS_SIDEBAR_FOOT_BLOCK = 16;`.
- Modify: `crates/arkos/src/effects/active.rs:187`: `scene()` takes `sidebar_end: Option<f32>`.
- Modify: `crates/arkos/src/main.rs`:
  - add the field `effect_sidebar_end: Option<f32>` beside `effect_divider` (lines 427 and 783);
  - pass it into `effects::active::scene` (line 4112).
- Modify: `crates/arkos/src/sidebar.rs`: report the end, as described in Step 5.
- Modify: `docs/agent-guide.md`, section 9, the paragraph listing the blocks.
- Test: `crates/arkos/src/effects/blocks.rs` (its `tests` module).

**Interfaces:**
- Produces: `blocks::Kind::SidebarFoot` (16), and `ARKOS_SIDEBAR_FOOT_BLOCK` in the preamble.
  - The block is reported whenever the sidebar shows and there is room below its last name.
  - Its rect runs from the last name's bottom to the sidebar's bottom, as wide as the sidebar block.
  - It has `edges` 0 and `seed` 5.0.
- Produces: `Scene { sidebar_end: Option<f32>, .. }`, the window y in points where the sidebar's last visible name ends.

- [ ] **Step 1: Write the failing tests** in `blocks.rs`'s tests module, using its existing helpers (`rect`, and a scene builder like the other tests'):

```rust
#[test]
fn a_sidebar_foot_spans_the_sidebar_below_its_last_name() {
    // content starts at x 240 under a 38 pt title bar and runs to y 800; the names end at 600.
    let out = blocks(&scene_with_sidebar_end(Some(600.0)), false);
    let foot: Vec<_> = out.iter().filter(|b| b.kind == Kind::SidebarFoot).collect();
    assert_eq!(foot.len(), 1);
    assert_eq!(foot[0].rect, rect(0.0, 600.0, 240.0, 200.0));
    assert_eq!((foot[0].edges, foot[0].seed), (0, 5.0));
}

#[test]
fn no_foot_is_reported_when_the_names_reach_the_bottom() {
    for end in [Some(800.0), Some(950.0), None] {
        assert!(!blocks(&scene_with_sidebar_end(end), false).iter().any(|b| b.kind == Kind::SidebarFoot), "{end:?}");
    }
}

#[test]
fn the_foot_is_as_wide_as_the_sidebar_on_screen_and_gone_without_one() {
    // Mid-slide the content starts at x 90; with no sidebar it starts at 0.
    let sliding = blocks(&scene_with(content_x(90.0), Some(600.0)), false);
    assert_eq!(sliding.iter().find(|b| b.kind == Kind::SidebarFoot).unwrap().rect.w, 90.0);
    assert!(!blocks(&scene_with(content_x(0.0), Some(600.0)), false).iter().any(|b| b.kind == Kind::SidebarFoot));
}

#[test]
fn the_foot_comes_before_the_panes_so_the_cap_never_drops_it() {
    let out = blocks(&scene_with_sidebar_end(Some(600.0)), false);
    let foot = out.iter().position(|b| b.kind == Kind::SidebarFoot).unwrap();
    let first_bar = out.iter().position(|b| b.kind == Kind::TabBar).unwrap();
    assert!(foot < first_bar);
}

#[test]
fn the_preamble_numbers_the_sidebar_foot_block_as_the_app_does() {
    assert_eq!(Kind::SidebarFoot as u32, 16);
    assert!(crate::effects::PREAMBLE.contains("ARKOS_SIDEBAR_FOOT_BLOCK = 16"));
}
```

Add `scene_with_sidebar_end` and `scene_with`/`content_x` as small helpers beside the module's existing scene setup: one pane, the given content x (default 240), title bar 38.

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos effects::blocks`
Expected: compile errors for `Kind::SidebarFoot` and `sidebar_end`.

- [ ] **Step 3: Implement**
  - Add the enum variant with a doc comment saying what the block is.
  - Add the `Scene` field.
  - Add the preamble constant, beside the other `_BLOCK` constants with a one-line comment.
  - Emit the block inside `blocks()`'s `if side > 1.0` branch, right after the divider. Emit it only when `sidebar_end` is `Some(y)` with `y < bottom - 1.0`. Its rect is `Rect { x: 0, y, w: side, h: bottom - y }`.
  - Thread `sidebar_end` through `active::scene` and the call in `main.rs`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `nice -n 15 cargo test -p arkos --bin arkos effects::`
Expected: all pass.

- [ ] **Step 5: Report the end from the sidebar.** The lowest names are always the Worktrees section's, because Worktrees sits under the Explorer and takes at least a third of the height. In `sidebar.rs`:
  - **Worktrees open:** add a zero-height `gpui::canvas` as the last child of `render_worktree_rows`'s scrolling `list`, after the rows and the section line. Because it sits inside the scroll, its `bounds.origin.y` is where the last name ends. It is below the sidebar's bottom when the list overflows.
  - **Worktrees closed:** add the same zero-height canvas after the `worktrees-header` in the `worktrees` div. That is the header's bottom.
  - Each canvas sets `root.effect_sidebar_end = Some(f32::from(bounds.origin.y))`, as the divider's canvas sets `effect_divider` (sidebar.rs:250-258).
  - `render` returns `None` while collapsed. Clear `effect_sidebar_end` to `None` at the top of `render` so a hidden sidebar reports nothing stale.

- [ ] **Step 6: Document the block.** In `docs/agent-guide.md` section 9, after the title group sentence, add a sentence on the foot (`ARKOS_SIDEBAR_FOOT_BLOCK`, no edges). It is the sidebar's empty space below its last visible name, through to the sidebar's bottom. It is reported whenever the sidebar shows and there is room, and it follows the lists as they grow, scroll, open and close.

- [ ] **Step 7: Check and commit**

Run: `nice -n 15 cargo check --workspace --all-targets`, which should show no warnings.

```bash
git add crates/arkos/src/effects/blocks.rs crates/arkos/effects/arkos.metal crates/arkos/src/effects/active.rs crates/arkos/src/main.rs crates/arkos/src/sidebar.rs docs/agent-guide.md
git commit -m "Effects: the sidebar reports the empty space below its last name"
```

---

### Task 2: `body_bars_alpha` keeps the bars inside tab bodies opaque

**Files:**
- Modify: `crates/arkos/src/effects/manifest.rs`: add the field `body_bars_alpha: Option<f32>`, its default `None`, a range check, and `Manifest::body_bars_alpha() -> f32`.
- Modify: `crates/arkos/src/effects/active.rs`: add `Style::body_bars_alpha: f32`, set in `Style::plain()` (1.0), in `style_at` and in the test literals at line 751.
- Modify: four sites, swapping `style.chrome_alpha` for `style.body_bars_alpha`:
  - `crates/arkos/src/pane_view.rs:1102` (the sheet strip);
  - `crates/arkos/src/pane_view.rs:1397` (the grid's banner row);
  - `crates/arkos/src/video_element.rs:303` (the video bar);
  - `crates/arkos/src/web_element.rs:233` (the web tab's address bar).
- Modify: `docs/agent-guide.md`, section 9: a key-table row after `chrome_alpha`.
- Test: the tests modules of `manifest.rs` and `active.rs`.

**Interfaces:**
- Produces: the manifest key `body_bars_alpha` (0.0 to 1.0; absent means `chrome_alpha`), and `Style::body_bars_alpha`.

- [ ] **Step 1: Write the failing tests**

```rust
// manifest.rs
#[test]
fn body_bars_alpha_follows_chrome_alpha_unless_given_and_is_refused_out_of_range() {
    let m = parse(&with("chrome_alpha = 0.2")).unwrap();
    assert_eq!((m.body_bars_alpha, m.body_bars_alpha()), (None, 0.2));
    assert_eq!(parse(&with("body_bars_alpha = 1.0")).unwrap().body_bars_alpha(), 1.0);
    let e = parse(&with("body_bars_alpha = 1.5")).unwrap_err();
    assert!(e.contains("body_bars_alpha must be 0.0 to 1.0"), "{e}");
}

// active.rs
#[test]
fn the_body_bars_follow_the_chrome_unless_the_manifest_says_otherwise() {
    let mut m = Manifest { chrome_alpha: 0.0, ..Manifest::default() };
    assert_eq!(style_at(&m, 1.0).body_bars_alpha, 0.0, "every existing effect is unchanged");
    m.body_bars_alpha = Some(1.0);
    assert_eq!((style_at(&m, 0.5).body_bars_alpha, style_at(&m, 1.0).body_bars_alpha), (1.0, 1.0));
    assert_eq!(Style::plain().body_bars_alpha, 1.0);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos body_bars`
Expected: compile errors for the missing field.

- [ ] **Step 3: Implement**
  - Follow `document_paper_alpha`'s pattern for the optional key and its accessor.
  - `style_at` sets `body_bars_alpha: lerp(1.0, manifest.body_bars_alpha(), f)`.
  - Swap the four sites.

- [ ] **Step 4: Run all the effects tests**

Run: `nice -n 15 cargo test -p arkos --bin arkos effects::`
Expected: pass.

- [ ] **Step 5: Document, check and commit.** The table row reads:

  | `body_bars_alpha` | 0.0 to 1.0 | `chrome_alpha` | The bars inside a tab's body over the wash: a grid's banner, a spreadsheet's sheet strip, a video's bar and a web tab's address bar. |

```bash
git add crates/arkos/src/effects/manifest.rs crates/arkos/src/effects/active.rs crates/arkos/src/pane_view.rs crates/arkos/src/video_element.rs crates/arkos/src/web_element.rs docs/agent-guide.md
git commit -m "Effects: body_bars_alpha keeps the bars inside a tab's body apart from the chrome"
```

---

### Task 3: The `overworld` effect

**Files:**
- Create: `crates/arkos/effects/overworld/effect.toml`.
- Create: `crates/arkos/effects/overworld/wash.metal`.
- Create: `crates/arkos/src/effects/overworld.rs`. This is the test-only mirror of the wash's constants and timing, headed like `space.rs`: "the constants and the arithmetic here are the shader's own; a change to one is made to both".
- Modify: `crates/arkos/src/effects/mod.rs`:
  - add `#[cfg(test)] pub mod overworld;`;
  - add a `BUILTIN` entry after `notebook`, with `glyphs: None` and `hidden: false`.
- Modify: the tests that list the built-ins, adding `overworld` last:
  - `crates/arkos/src/effects/manifest.rs:555`;
  - `crates/arkos/src/effects/cli.rs:494` and `:504`;
  - any other test `cargo test` shows failing for the new entry.
- Modify: `crates/arkos/src/effects/cli.rs:622`: add `("overworld", "night")` to the compile test's list.

**Interfaces:**
- Consumes: `ARKOS_SIDEBAR_FOOT_BLOCK` (Task 1) and `body_bars_alpha` (Task 2).
- Produces: the built-in effect `overworld`, and the mirror functions below, which the shader implements with the same constants.

**The manifest** (`effect.toml`) has exactly these values:
- `name = "overworld"`
- `description = "A night overworld in muted pixel art at the foot of the sidebar: stars, a castle, pines and fireflies."`
- `wash = "wash.metal"`
- `frame_rate = 10`
- `scale = 1.0`
- `ink_borders = false`
- `highlights = "effect"`
- `menus = "app"`
- `paper_alpha = 1.0`
- `chrome_alpha = 0.0`
- `body_bars_alpha = 1.0`
- `active_tab_alpha = 0.0`
- `text_contrast = 0.0`

Comment each key as `material/effect.toml` does.

**What the wash paints, block by block.** Every colour comes from `u.colors`. "Muted" means `mix(c, sidebar, 0.45)`.

- **Title bar:** `ARKOS_TITLE_BAR` colour, plus muted stars in the bar's two ends. Each star is one art pixel. A star is allowed only where `star_clear` passes against every `ARKOS_TITLE_GROUP_BLOCK`.
- **Sidebar:** `ARKOS_SIDEBAR` colour. Inside `scene_band` of the `ARKOS_SIDEBAR_FOOT_BLOCK` it draws the scene, laid out from the band's width and bottom-anchored. From bottom to top:
  - grass (muted `ARKOS_ANSI+2`, the terminal green) with a winding path;
  - pines near both edges;
  - low hills (the sidebar a step darker);
  - a centred castle silhouette, with windows in the full `ARKOS_FOCUS_RING` colour and a muted magenta pennant (`ARKOS_ANSI+5`);
  - a dithered sky band (sidebar mixed toward `ARKOS_BANNER`) with stars and a crescent moon.

  The scene is cut from the top when the band is shorter than the full scene, so the sky goes first and the ground last. Fireflies are the full terminal yellow (`ARKOS_ANSI+3`).
- **Tab bars:** `ARKOS_TAB_BAR` colour.
- **Active tab:** `ARKOS_TAB_ACTIVE` colour, with a lantern 3 art pixels wide and 4 tall, 1 art pixel in from the block's left and centred on its height. It has a muted body and a yellow flame that glows softly.
- **Pane bodies:** `ARKOS_WINDOW_BG` colour. Nothing else is drawn there.
- **Row and tab highlights:** filled as the app fills them, scaled by the block's `value` (its fade):
  - a selected row in `ARKOS_TAB_ACTIVE`;
  - a hovered row or tab in `ARKOS_TAB_INACTIVE`.

  Read `sidebar.rs` `render_rows` and `render_worktree_rows` and pane_view's hovered tab for the colours they use without an effect, and match them.
- **Everything else** (window background, divider): `ARKOS_WINDOW_BG`. The app still draws the borders.

- [ ] **Step 1: Write the mirror's failing tests** in `overworld.rs`:

```rust
#[test]
fn the_scene_fills_the_foot_up_to_its_full_height_from_the_bottom() {
    let band = scene_band(Rect { x: 0.0, y: 400.0, w: 240.0, h: 300.0 }).unwrap();
    assert_eq!(band, Rect { x: 0.0, y: 562.0, w: 240.0, h: 138.0 }, "46 art pixels, the most under 140 points");
}

#[test]
fn a_shorter_foot_cuts_the_scene_from_the_top_in_whole_art_pixels() {
    let band = scene_band(Rect { x: 0.0, y: 600.0, w: 240.0, h: 91.0 }).unwrap();
    assert_eq!((band.y, band.h), (691.0 - 90.0, 90.0));
}

#[test]
fn under_fifty_points_of_room_there_is_no_scene() {
    assert_eq!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 49.0 }), None);
    assert!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 50.0 }).is_some());
}

#[test]
fn stars_keep_clear_of_the_title_bar_s_words_and_icons() {
    let words = Rect { x: 94.0, y: 10.0, w: 300.0, h: 18.0 };
    assert!(!star_clear((94.0 - STAR_CLEAR + 1.0, 12.0), &[words]));
    assert!(star_clear((94.0 - STAR_CLEAR - 1.0, 12.0), &[words]));
    assert!(!star_clear((200.0, 12.0), &[words]));
}

#[test]
fn at_time_zero_everything_is_lit() {
    assert_eq!(dimmed_window(0.0), None);
    for i in 0..FIREFLIES {
        assert_eq!(firefly(i, 0.0).1, 1.0, "firefly {i}");
    }
    for seed in 0..40 {
        assert_eq!(twinkle(seed as f32, 0.0), 1.0, "star {seed}");
    }
}

#[test]
fn a_castle_window_dims_every_ten_to_twenty_seconds_one_at_a_time() {
    let starts = dim_starts_until(400.0);
    assert!(starts.len() >= 18);
    for w in starts.windows(2) {
        assert!((10.0..=20.0).contains(&(w[1] - w[0])), "{w:?}");
    }
    // Each dims for DIM_FOR and comes back.
    let s = starts[0];
    assert!(dimmed_window(s + DIM_FOR * 0.5).is_some() && dimmed_window(s + DIM_FOR + 0.01).is_none());
}

#[test]
fn a_firefly_is_gone_before_it_leaves_the_scene() {
    for i in 0..FIREFLIES {
        for k in 0..200 {
            let (rise, alpha) = firefly(i, k as f32 * 0.1);
            assert!((0.0..=1.0).contains(&rise));
            if rise > 0.95 { assert!(alpha < 0.05, "firefly {i} at {k}"); }
        }
    }
}
```

The mirror's constants and signatures:
- `pub const ART_PX: f32 = 3.0;`
- `pub const SCENE_MAX_H: f32 = 140.0;`
- `pub const SCENE_MIN_H: f32 = 50.0;`
- `pub const STAR_CLEAR: f32 = 12.0;`
- `pub const DIM_CYCLE: f32 = 15.0;`
- `pub const DIM_SLACK: f32 = 5.0;`
- `pub const DIM_FOR: f32 = 0.8;`
- `pub const FIREFLIES: usize = 3;`
- `pub const CASTLE_WINDOWS: usize = 5;` (the scene's lit windows, which the shader draws)
- `pub fn scene_band(foot: Rect) -> Option<Rect>`:
  - `None` when `foot.h < SCENE_MIN_H`;
  - otherwise the height is `min(foot.h, SCENE_MAX_H)` floored to a whole number of `ART_PX`, and the band is anchored to the foot's bottom.
- `pub fn star_clear(p: (f32, f32), groups: &[Rect]) -> bool`: false inside any group grown by `STAR_CLEAR` on every side.
- `pub fn dimmed_window(t: f32) -> Option<usize>`:
  - Event `k ≥ 1` starts at `k * DIM_CYCLE + (hash(k) - 0.5) * DIM_SLACK`, so consecutive starts are 10 to 20 seconds apart.
  - It returns `Some(hash2(k) % CASTLE_WINDOWS)` during `[start, start + DIM_FOR)`.
  - Cycle 0 has no event, so time 0 is lit.
- `pub fn dim_starts_until(t: f32) -> Vec<f32>`: a test helper listing the event starts.
- `pub fn firefly(i: usize, t: f32) -> (f32, f32)`: `(rise 0..1 through the scene, alpha)`.
  - Its phase puts `t = 0` at full alpha.
  - Alpha falls to 0 over the last 15% of the rise.
- `pub fn twinkle(seed: f32, t: f32) -> f32`: `0.75 + 0.25 * cos(2π t / period(seed))`, with a period of 4 to 9 seconds, so every star is at 1.0 at `t = 0`.
- The hash is the one `space.rs` mirrors from its shader. Reuse its formula.

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos effects::overworld`
Expected: compile errors, since nothing exists yet.

- [ ] **Step 3: Implement the mirror.** Make the tests pass.

- [ ] **Step 4: Write the manifest and the wash.**
  - `wash.metal` implements the same constants and functions in Metal, then paints the blocks as listed above.
  - Snap every art pixel: `floor((p - band.origin) / ART_PX)` on the point position, so pixel edges land on whole points. At `scale = 1.0` on a Retina screen a point is 2 drawable pixels, so the edges stay crisp.
  - Draw the scene in band-local art-pixel coordinates, so it is laid out from the band's width and bottom. The castle is centred; pines sit 2 to 3 art pixels from each edge.
  - The art may be built from rectangles and per-row width tables, as the mockup's `scene()` builds it in `overlap.html`.
  - Add the `BUILTIN` entry and the `mod` line, and update the listing tests.

- [ ] **Step 5: Compile, check and measure**

Run, in order:
1. `nice -n 15 cargo test -p arkos --bin arkos effects::` (pass; the compile test now covers `overworld`).
2. `nice -n 15 cargo build -p arkos`.
3. `/Users/csarko/Projects/ide-arkos/target/debug/arkos effect check crates/arkos/effects/overworld`. Expected: `ok: overworld` and `highlights: effect`.
4. `/Users/csarko/Projects/ide-arkos/target/debug/arkos effect bench overworld`. Expected: `within budget`.

- [ ] **Step 6: Look at it.**
  1. Open the dev sandbox: `target/debug/arkos sandbox <scratch folder>`, then `target/debug/arkos sandbox theme use timo-purple` and `target/debug/arkos sandbox effect use overworld`.
  2. Check it against `overworld-v2.html`, A2: muted, crisp, with nothing behind names.
  3. If your session cannot capture the screen, say so in the report, and leave the look to the Status rows in Task 4.
  4. Stop the sandbox afterwards with `target/debug/arkos sandbox stop`.

- [ ] **Step 7: Check and commit**

```bash
git add crates/arkos/effects/overworld crates/arkos/src/effects/overworld.rs crates/arkos/src/effects/mod.rs crates/arkos/src/effects/manifest.rs crates/arkos/src/effects/cli.rs
git commit -m "Effects: overworld, a muted pixel-art night overworld at the foot of the sidebar"
```

---

### Task 4: Timo Purple recommends `overworld`, and the docs say so

**Files:**
- Modify: `crates/arkos/themes/timo-purple.toml`: add `[effect]` and `name = "overworld"` after `appearance`, as `matrix.toml:13` does.
- Modify: `crates/arkos/src/theme.rs`: add a test beside `timo_purple_is_built_in_and_its_accent_is_the_cyan`.
- Modify: `docs/guide.md` (the effects paragraph, around line 515, which names each theme's recommendation and says Timo Purple draws none).
- Modify: `docs/agent-guide.md` section 9 ("Five are built in", and each theme's recommendation).
- Modify: `docs/development.md`, adding Status rows.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn timo_purple_recommends_the_overworld_effect() {
    assert_eq!(builtin("timo-purple").unwrap().recommended_effect(), Some("overworld"));
}
```

- [ ] **Step 2: Run it and see it fail; add the `[effect]` table; run it and see it pass**

Run: `nice -n 15 cargo test -p arkos --bin arkos timo_purple`

- [ ] **Step 3: Run the contrast checks.** Each must exit 0. Never loosen a floor; if one fails, fix the effect.

```bash
T=/Users/csarko/Projects/ide-arkos/target/debug/arkos
nice -n 15 cargo build -p arkos
$T theme contrast timo-purple --effect overworld
$T theme contrast peach-watercolor --effect overworld
$T theme contrast navy --effect overworld
```

- [ ] **Step 4: Update the docs.**
  - **The guide and the agent guide:** six built-in effects. Timo Purple recommends `overworld`, described in one clause as "a muted pixel-art night overworld at the foot of the sidebar, a lantern on the active tab". Navy alone draws plain.
  - **`development.md` Status:** add these rows under the effects rows:
    - effects: under Timo Purple, overworld draws a muted night scene at the foot of the sidebar (stars, a crescent moon, a castle with cyan windows, pines, grass and a path) with crisp 3 point pixels on Retina and on a 1x screen, a few stars at the title bar's ends clear of its words, and a lantern on each pane's active tab; every tab body is plain Timo Purple
    - effects: under overworld the Explorer's selected and hovered rows, the Worktrees rows and the hovered tab are filled as without an effect
    - effects: under overworld, as the Worktrees list grows the scene is cut from the top, and with under about 50 points left below the last name it is gone; with Worktrees closed there is no scene; no name ever sits on art
    - effects: under overworld, Cmd+B slides the sidebar out with the scene and back; a narrow sidebar keeps the castle centred and the pines at its edges
    - effects: under overworld the stars twinkle slowly, a castle window dims every 10 to 20 seconds, fireflies rise from the grass; under Reduce Motion it is a still frame with everything lit
    - effects: under overworld a grid's banner, a spreadsheet's sheet strip, a video's bar and a web tab's address bar keep their own colours
    - effects: overworld on Peach Watercolor reads as an intended dusk scene, not a broken one

- [ ] **Step 5: Run the whole suite, check, and commit**

Run the workspace tests (Global Constraints), then `nice -n 15 cargo check --workspace --all-targets`.

```bash
git add crates/arkos/themes/timo-purple.toml crates/arkos/src/theme.rs docs/guide.md docs/agent-guide.md docs/development.md
git commit -m "Timo Purple recommends overworld"
```
