# Matrix City Sheds and the Cart's Line Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sidewalk sheds, some with scaffolding above, on towers beside the road in the Matrix city; the food cart's line kept on the sidewalk.

**Architecture:** All drawing is in the runic wash (`crates/arkos/effects/runic/wash.metal`, `city_scene`). The tests in `crates/arkos/src/effects/runic.rs` mirror the shader's geometry in Rust, read its constants with `runic_constant`, and pin the mirrored lines with `wash.contains(line)` checks.

**Tech Stack:** Metal Shading Language, Rust tests.

**Spec:** `docs/superpowers/specs/2026-11-17-runic-world-design.md`, sections "Sidewalk sheds and scaffolding" and "The food cart's line keeps to the sidewalk".

## Global Constraints

- The geometry already in the shader: GRID_X 0.9, GRID_Z 1.6, ROAD_CELLS 1 (road from -0.9 to 0.9), SIDEWALK_IN 0.02 and SIDEWALK_W 0.32 (so the curb is at |x| 0.88 and the road's sidewalk runs to |x| 1.20), PERSON_H 0.16, and window floors 0.25 apart.
- A tower at lot (kx, m): `tw = GRID_X * (0.5 + 0.35 * hash2(cell + (5, 11)))`, `td = GRID_Z * (0.35 + 0.35 * hash2(cell + (9, 2)))`, `th = 0.6 + (TOWER_MAX - 0.6) * pow(hash2(cell + (4, 7)), 1.6)`, centre `tx = (kx + 0.5) * GRID_X`, near face `zf = zc + 0.5 * GRID_Z - 0.5 * td`.
- Every colour comes from `u.colors`, as the scene's other lines do (`green`, `white_c`).
- The wash's GPU cost was 4.9 to 5.0 ms a frame at 1200 by 800 under load. Say what this adds.
- Comments plain prose, no em-dashes. Tests named as sentences, written first.

## Review Focus

- A cart on a lot beside a cross street: the line runs along z, and must stay inside its lot's z span, off the cross street (Task 1 test).
- A tower so wide its road face sits almost at the curb (tw near 0.765, face at |x| 0.9675): the shed is then very narrow. Its posts must still be on the sidewalk, not in the road (Task 2 test).
- A short tower (th 0.6) with scaffolding: the scaffolding stops below the roof (Task 2 test).
- A tower on the road's left (kx = -2) mirrors the right one's shed (Task 2 test covers both sides).
- The 1x external screen: the city is supersampled there. New thin lines must not flash (hand check, `city_blinks` test still passing).

---

### Task 1: The cart's line runs along the sidewalk

**Files:**
- Modify: `crates/arkos/effects/runic/wash.metal` (the `kind == 1` branch of `small_lot_at`, near line 1574)
- Test: `crates/arkos/src/effects/runic.rs` (`mod tests`)

**Interfaces:**
- Produces: shader constants `CART_W = 0.16` (replacing `cw`), `CART_FRONT = 0.1` (the line's distance from the cart's road side) and `CART_GAP = 0.13` (one person to the next along the line). Shader lines:
  - `float qx = xm + side * (CART_W + CART_FRONT);`
  - `float qz = wz - 0.05 + (float(i) - step) * CART_GAP - leave * 0.5;`
- Produces (test mirror): `fn cart_line_at(kx: i32, i: i32, step: f32, leave: f32) -> (f32, f32)`, giving x across and z relative to the lot's near edge.

- [ ] **Step 1: Write the failing tests.**

`the_food_cart_s_line_stays_on_the_sidewalk_by_the_road`: for kx in {1, -2}, every i in 0..4, step and leave over 0..=1 in 0.05 steps. Assert `|x| > 0.88 + 0.03` (off the road, a body clear of the curb) and `|x| < 0.88 + SIDEWALK_W`.

`the_food_cart_s_line_stays_in_its_lot_further_out`: for kx in 2..7 and -7..-2, assert x is in `[kx*GRID_X, (kx+1)*GRID_X]` and z is in `[0, GRID_Z]`.

Add a wash line check for the two lines above.

- [ ] **Step 2: Run them; expect FAIL**

Run: `nice -n 15 cargo test -p arkos --bin arkos effects::runic::tests::the_food_cart`

The line checks fail, and so do the geometry checks against the old formula if mirrored first.

- [ ] **Step 3: Change the shader to the two lines above.** The cart's own drawing uses `CART_W`; nothing else moves.
- [ ] **Step 4: Run the runic tests; expect PASS** (`effects::runic`).
- [ ] **Step 5: Commit** "Matrix: the food cart's line keeps to the sidewalk"

### Task 2: Sidewalk sheds and scaffolding on towers beside the road

**Files:**
- Modify: `crates/arkos/effects/runic/wash.metal` (constants block near line 540; the tower loop in `city_scene` near line 2243, drawn after the tower's own lines so nearer towers still cover it)
- Test: `crates/arkos/src/effects/runic.rs`
- Modify: `docs/superpowers/specs/2026-11-17-runic-world-design.md`. Fix the slip "(over sheds only, or scaffolding only)" to read "over sheds alone or scaffolding alone", and add the measured cost.
- Modify: `docs/development.md` Status, a new row.

**Interfaces:**
- Produces shader constants:
  - `SHED_SHARE = 0.3`: towers beside the road with a shed.
  - `SHED_H = 0.22`: the deck's height.
  - `SHED_POST = 0.2`: posts along z.
  - `SHED_IN = 0.03`: the posts' line inside the curb.
  - `SCAFFOLD_SHARE = 0.33`: sheds with scaffolding.
  - `SCAFFOLD_FLOOR = 0.25`.
  - `SCAFFOLD_FLOORS = 4.0`: the most floors above the deck.
- Produces shader selection lines:
  - `bool beside_road = kx == ROAD_CELLS || kx == -ROAD_CELLS - 1;`
  - `bool shed = beside_road && hash2(cell + float2(61.0, 13.0)) < SHED_SHARE;`
  - `bool scaffold = shed && hash2(cell + float2(67.0, 19.0)) < SCAFFOLD_SHARE;`
- Produces (test mirror): `struct Shed { post_x: f32, face_x: f32, z0: f32, z1: f32, deck: f32, top: f32 }` (|x| values on the tower's side; `top == deck` without scaffolding) and `fn shed_of(kx: i32, m: i32) -> Option<Shed>`.

- [ ] **Step 1: Write the failing tests.**
  - `about_three_in_ten_towers_beside_the_road_wear_a_sidewalk_shed`: over m in 0..400 on both sides, among built lots (`hash2(cell + (17, 3)) <= TOWER_ON`), the share with `Some` is in 0.2..0.4. Lots with |kx| beyond the road's neighbours always give `None`.
  - `a_shed_s_posts_stand_on_the_sidewalk_never_in_the_road`: for every shed, `post_x == 0.88 + SHED_IN`, `post_x < face_x`, and z0 and z1 equal the tower's `zf` and `zf + td` (relative to the lot's near edge).
  - `people_walk_under_a_shed_s_deck`: `SHED_H >= PERSON_H + 0.04`.
  - `about_one_in_three_sheds_has_scaffolding_never_above_the_roof`: the share in 0.2..0.47. With scaffolding, `top = min(th - 0.05, deck + SCAFFOLD_FLOORS * SCAFFOLD_FLOOR)` and `top <= th`.
  - A wash line check for the three selection lines and the `top` line.
- [ ] **Step 2: Run them; expect FAIL** (`effects::runic::tests`).
- [ ] **Step 3: Draw it in the tower loop.**
  - Skip unless `shed` and q lies within the shed's screen box. That box comes from projecting its corners (posts' line, face, z0, z1, ground, top), padded 1 point.
  - The deck: a rail along z at `deck` height on the posts' line and one on the face, plus one across at z0 from the posts' line to the face.
  - The posts: verticals from the ground to `deck` at z0 + k * SHED_POST up to z1.
  - The scaffolding: standards on the face plane at the same z steps, from `deck` to `top`, and a ledger along z at each floor up to `top`.
  - All lines go through `seg_dist` in screen space, lit with `light_on` in `green`: the deck at 0.55, posts and standards at 0.4, ledgers at 0.3, scaled by the tower's `seen`.
  - Sign: the tower's road side is `face = tx - sign(tx) * tw * 0.5` and the posts' line `sign(tx) * (0.88 + SHED_IN)`.
- [ ] **Step 4: Run the runic tests and the stills; expect PASS.**
  - Run: `nice -n 15 cargo test -p arkos --bin arkos effects::`.
  - `on_a_1x_screen_the_city_s_edges_do_not_flash` must still pass.
- [ ] **Step 5: Measure.**
  - Run `nice -n 15 cargo test -p arkos --bin arkos effects::bench -- --ignored` before (on main) and after.
  - Write the difference into the design section. If it adds over 0.3 ms, say so to the user before going on.
- [ ] **Step 6: Check contrast.** Run `arkos theme contrast matrix --effect runic` (from the sandbox build) and expect it to pass as before.
- [ ] **Step 7: Add a Status row.** "Matrix sheds: in the city, some towers beside the road wear a pipe shed over the sidewalk with posts at the curb and people walking under it; some of those have scaffolding a few floors up their road side; none in the road; smooth on a 1x screen".
- [ ] **Step 8: Commit** "Matrix: sidewalk sheds, some with scaffolding, on towers beside the road"

### Task 3: Finish

- [ ] Run `nice -n 15 cargo test --workspace`; expect all pass.
- [ ] Run `nice -n 15 cargo check --workspace --all-targets`; expect no warnings.
- [ ] Open a sandbox preview on the Matrix theme for the user's look.
