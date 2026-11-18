# Matrix City Water Towers, Steam and Subway Entrances Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rooftop water towers, manhole steam in the road and subway entrances at the road's corners in the Matrix city.

**Architecture:** All drawing is in the runic wash (`crates/arkos/effects/runic/wash.metal`, `city_scene`). Tests in `crates/arkos/src/effects/runic.rs` mirror the shader's choices in Rust, read constants with `runic_constant`, and pin mirrored lines with `wash.contains(line)`.

**Tech Stack:** Metal Shading Language, Rust tests.

**Spec:** `docs/superpowers/specs/2026-11-17-runic-world-design.md`, section "Water towers, steam and subway entrances".

## Global Constraints

- Geometry already in the shader: GRID_X 0.9, GRID_Z 1.6, CROSS_EVERY 4 (block 6.4), ROAD_CELLS 1, curb at |x| 0.88, the road's sidewalk to |x| 1.20, PERSON_H 0.16, car lanes at |x| 0.45, bike lanes at |x| 0.702.
- A tower at lot (kx, m): `tw`, `td`, `th`, `tx`, `zf` as in the tower loop; a lot wears a shed when `beside_road && lot_built && hash2(cell + (61, 13)) < SHED_SHARE`.
- Walkers' base |x|: `0.96 + (h - 0.22) * WALKER_SPREAD` (0.894 to 1.194); strollers' `0.96 + (0.1 + 0.6 h) * WALK_W`.
- Colours from `u.colors` only. Comments plain prose, no em-dashes. Tests named as sentences, written first.
- The wash measured 4.92 to 5.02 ms a frame at 1200 by 800. Say what this adds; over 0.3 ms goes to the user first.

## Review Focus

- A walker who is standing (waiting at the corner, or a jaywalker standing back up to WAIT_DEPTH) beside an entrance: pressed aside too (Task 3 test over every base x and z).
- A transfer of the city's data landing on a walker beside an entrance: it lands where the walker is drawn (Task 3 pins the shared function in `data_end` and `small_end`).
- A water tower on a short tower near the horizon line (th near 0.9, camera height 1): its roof is seen almost edge on; the tank still sits on the roof (Task 1 geometry test).
- A steam plume next to a crosswalk: never over it (Task 2 test).
- The 1x screen: `on_a_1x_screen_the_city_s_edges_do_not_flash` still passing.

---

### Task 1: Rooftop water towers

**Files:** Modify `wash.metal` (constants near SHED_*; tower loop after the tower's `light_on`); Test `runic.rs`.

**Interfaces:**
- Produces constants `WT_SHARE 0.45`, `WT_MIN 0.9`, `WT_MAX 3.0`, `WT_R 0.045`, `WT_LEGS 0.05`, `WT_TANK 0.09`, `WT_CONE 0.04`.
- Produces selection line `bool water = th >= WT_MIN && th <= WT_MAX && hash2(cell + float2(71.0, 23.0)) < WT_SHARE;` and its place lines `float wtx = tx + (hash2(cell + float2(73.0, 29.0)) - 0.5) * (tw - 2.0 * WT_R - 0.04);`, `float wtz = zf + WT_R + 0.02 + hash2(cell + float2(79.0, 31.0)) * (td - 2.0 * WT_R - 0.04);`.
- Test mirror `fn water_tower_of(kx, m) -> Option<(f32, f32, f32)>` (x, z in lot, roof height).

- [ ] Write failing tests: `about_four_in_ten_middling_towers_wear_a_water_tower_and_no_tall_or_short_one` (share in 0.35..0.55 among towers with th in WT_MIN..WT_MAX, none outside); `a_water_tower_stands_on_its_roof_never_over_the_edge` (|wtx - tx| + WT_R <= tw/2, zf <= wtz - WT_R, wtz + WT_R <= zf + td); `a_water_tower_reads_as_a_tower_s_edges_do` (lines pinned, outline lit 0.8). Run, expect FAIL.
- [ ] Draw it: extend the tower's screen cull upward by the water tower's height; after the tower's lines, fill the tank's screen rectangle dark (bg * 0.75) and light its sides, bottom, two hoops, three legs and the cone's two edges with `seg_dist`, at 0.8 in `mix(green, white_c, 0.15)`, scaled by `seen`.
- [ ] Run `effects::runic`; expect PASS. Commit "Matrix: water towers on the city's middling roofs".

### Task 2: Manhole steam

**Files:** Modify `wash.metal` (constants; after the road's traffic, before the towers loop); Test `runic.rs`.

**Interfaces:**
- Produces constants `STEAM_SHARE 0.7`, `STEAM_H 0.5`, `STEAM_W0 0.08`, `STEAM_W1 0.3`, `STEAM_DRIFT 0.15`, `STEAM_LIGHT 0.4`.
- Produces `float4 steam_vent(int j)`: x, world z, whether it breathes (1 or 0), its phase. Place lines: x `side * (0.15 + 0.45 * hash2(float2(float(j), 83.0)))`, z `float(j) * block + GRID_Z + WALK_AT + 0.3 + hash2(float2(float(j), 89.0)) * (block - GRID_Z - 2.0 * WALK_AT - 0.6)`.
- Test mirror `fn steam_vent(j) -> Option<(f32, f32)>`.

- [ ] Write failing tests: `about_seven_blocks_in_ten_breathe_steam_from_a_manhole` (share 0.6..0.8 over j in 0..400); `steam_rises_from_a_traffic_lane_never_a_crosswalk_or_a_bike_lane` (|x| + STEAM_W0/2 < 0.702 - 0.05, the plume's widest reach plus drift stays inside the road, z local clear of both crosswalks by 0.2); `steam_is_about_three_people_tall` (STEAM_H in 2.5..3.5 PERSON_H). Run, expect FAIL.
- [ ] Draw it: for the blocks between Z_NEAR and 0.6 Z_FAR, cull by the plume's screen box, then density = envelope (across and up) times `fbm` of plume coordinates rising with t, its strength puffing with `0.6 + 0.4 * sin(t * 0.7 + phase)`; `col = light_on(mix(col, bg, d * 0.25), mix(green, white_c, 0.5), d * STEAM_LIGHT * seen, u.dark)`.
- [ ] Run `effects::runic`; expect PASS. Commit "Matrix: steam from the road's manholes".

### Task 3: Subway entrances, with the walkers keeping out of them

**Files:** Modify `wash.metal` (constants; a `subway_entrance` test, a `sidewalk_x` function used by the road's walkers and strollers in `city_scene`, `data_end` and `small_end`; the drawing in the lot loop before the built check); Test `runic.rs`.

**Interfaces:**
- Produces constants `ENTRANCE_SHARE 0.33`, `ENTRANCE_AT 0.9`, `ENTRANCE_LEN 0.24`, `ENTRANCE_W 0.07`, `ENTRANCE_RAIL 0.08`, `ENTRANCE_LAMP 0.22`, `ENTRANCE_RED 0.25`, `ENTRANCE_EASE 0.15`, `ENTRANCE_CLEAR 0.03`.
- Produces `bool entrance_at(int kx, int m)`: kx beside the road, m % 4 is 1 (the far corner of the cross street before it) or 3 (the near corner of the one after), the lot wears no shed, and `hash2(float2(kx, m) + float2(97.0, 37.0)) < ENTRANCE_SHARE`. The railing's z span in the lot: from `ENTRANCE_AT` past the cross street's curb, `ENTRANCE_LEN` long.
- Produces `float sidewalk_x(float side, float ax, float zw)`: signed x for a person whose base |x| is ax at world z zw; beside an entrance (eased over ENTRANCE_EASE) ax is pressed from [0.894, 1.194] into [0.88 + 0.01 + ENTRANCE_W + ENTRANCE_CLEAR, 1.194].
- Test mirrors `entrance_at`, `sidewalk_x`.

- [ ] Write failing tests: `about_a_third_of_the_road_s_corners_have_a_subway_entrance_never_under_a_shed`; `a_subway_entrance_stands_at_the_curb_clear_of_the_corner_s_crowd` (its z span starts past WALK_AT + 0.12 + WAIT_ALONG and ends inside the lot; its x on the sidewalk); `no_walker_or_stroller_ever_walks_through_a_subway_entrance` (for every base ax a walker or stroller can have and z over the span in 0.01 steps, `sidewalk_x` is at least ENTRANCE_CLEAR past the railing and inside the sidewalk); `the_city_s_data_lands_on_a_walker_where_it_is_drawn` (`sidewalk_x(` in all four places, the old inline walker x lines gone); `most_subway_globes_are_green_some_red`. Run, expect FAIL.
- [ ] Implement `entrance_at` and `sidewalk_x`, and route the walkers, strollers, `data_end` and `small_end` through it.
- [ ] Draw it: rails along z at both sides and across the closed end at ENTRANCE_RAIL, posts at the four corners, three faint step lines inside fading deeper, two lamp posts at the open end with a globe (`car_light`) green or red (`u.colors[ARKOS_ANSI + 1]`).
- [ ] Run `effects::`; expect PASS. Commit "Matrix: subway entrances at the road's corners".

### Task 4: Finish

- [ ] Bench before (main) and after: `nice -n 15 cargo test -p arkos --bin arkos effects::bench -- --ignored --nocapture`; write the cost into the design section.
- [ ] Stills at a few times to check by eye; `nice -n 15 cargo test --workspace`; `nice -n 15 cargo check --workspace --all-targets` with no warnings.
- [ ] Status row in `docs/development.md`; commit; open the sandbox.
