//! The built-in `space` effect's event schedule and its pane edges' query bars, mirrored from its
//! wash shader (`effects/space/wash.metal`) so the timings can be tested without a GPU. The constants and the
//! arithmetic here are the shader's own; a change to one is made to both.

/// Seconds from one shooting star's cycle to the next; each starts up to `METEOR_SLACK` into its
/// cycle, so the gap between two is 8 to 16 seconds.
pub const METEOR_CYCLE: f32 = 12.0;
pub const METEOR_SLACK: f32 = 4.0;
/// How long a shooting star takes to cross.
pub const METEOR_FLIGHT: f32 = 0.8;
/// Where the clock's zero falls in the shooting stars' schedule: past the latest a first one
/// could have finished, so a frozen clock shows none mid-flight.
pub const METEOR_OFFSET: f32 = 6.0;

/// A salt that gives the shooting stars hashes of their own.
pub const METEOR_SALT: u32 = 0x9E37_79B9;

/// A hash of `n` in 0 to 1, in integer arithmetic (PCG's output step) so the shader and this
/// agree to the bit: the top 24 bits, times a power of two, are exact in a float.
pub fn hash01(n: u32) -> f32 {
    let mut h = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
    h ^= h >> 22;
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Whether a shooting star is crossing at `t` seconds of the effect's clock.
pub fn meteor_visible(t: f32) -> bool {
    let tau = t + METEOR_OFFSET;
    let n = (tau / METEOR_CYCLE).floor();
    let x = tau - n * METEOR_CYCLE - METEOR_SLACK * hash01((n as u32) ^ METEOR_SALT);
    (0.0..METEOR_FLIGHT).contains(&x)
}

/// Seconds a bar takes to sweep an edge at speed 1. The bars copy Material UI's LinearProgress,
/// variant "query": its two indeterminate bars, run from the far end. Material sweeps for 60% of
/// its 2.1 s cycle.
pub const QUERY_SWEEP: f32 = 1.26;
/// Seconds at speed 1 from one bar's start to the next's, Material's 1.15 s.
pub const QUERY_STEP: f32 = 1.15;
/// The long bar's and the short bar's CSS `left` and `right`, as fractions of the edge, where
/// a sweep starts.
pub const QUERY_FROM: [[f32; 2]; 2] = [[-0.35, 1.0], [-2.0, 1.0]];
/// Each bar's CSS `left` and `right` where its sweep ends.
pub const QUERY_TO: [[f32; 2]; 2] = [[1.0, -0.9], [1.07, -0.08]];
/// Each bar's easing, as a CSS `cubic-bezier(x1, y1, x2, y2)`.
pub const QUERY_EASE: [[f32; 4]; 2] = [[0.65, 0.815, 0.735, 0.395], [0.165, 0.84, 0.44, 1.0]];

/// The shimmers' schedule: one shimmer a slot, starting up to `SHIMMER_SLACK` into it, so the
/// gap between two is 1.5 to 5.5 seconds. A shimmer can run on into the next slot, overlapping
/// the next shimmer, but even the slowest of three bars ends before the slot after that.
pub const SHIMMER_SLOT: f32 = 3.5;
pub const SHIMMER_SLACK: f32 = 2.0;
/// Where the clock's zero falls: before the first shimmer starts, so a frozen clock shows no bar.
pub const SHIMMER_OFFSET: f32 = 0.1;
/// The slowest and fastest a shimmer runs, as multiples of Material's pace.
pub const SHIMMER_SPEED: [f32; 2] = [0.8, 1.5];
/// The chance an edge takes part in a given shimmer.
pub const SHIMMER_EDGE_CHANCE: f32 = 0.55;
/// A salt that gives the shimmers hashes of their own.
pub const SHIMMER_SALT: u32 = 0xC2B2_AE35;

/// One shimmer: its hash, the seconds since it started (negative before), its speed and its
/// number of bars (1 to 3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shimmer {
    pub id: u32,
    pub elapsed: f32,
    pub speed: f32,
    pub bars: u32,
}

/// The shimmer of the slot `t` seconds of the effect's clock falls in.
pub fn shimmer_at(t: f32) -> Shimmer {
    let tau = t + SHIMMER_OFFSET;
    in_slot((tau / SHIMMER_SLOT).floor(), tau)
}

/// The shimmers that can be playing at `t`: the previous slot's, still finishing (none in the
/// first slot), and the current slot's.
pub fn shimmers_at(t: f32) -> [Option<Shimmer>; 2] {
    let tau = t + SHIMMER_OFFSET;
    let n = (tau / SHIMMER_SLOT).floor();
    [(n >= 1.0).then(|| in_slot(n - 1.0, tau)), Some(in_slot(n, tau))]
}

/// Slot `n`'s shimmer, `tau` seconds into the shimmers' schedule.
fn in_slot(n: f32, tau: f32) -> Shimmer {
    let id = (n as u32) ^ SHIMMER_SALT;
    let elapsed = tau - n * SHIMMER_SLOT - SHIMMER_SLACK * hash01(id);
    let speed = SHIMMER_SPEED[0] + (SHIMMER_SPEED[1] - SHIMMER_SPEED[0]) * hash01(id.wrapping_add(1));
    let bars = 1 + ((hash01(id.wrapping_add(2)) * 3.0) as u32).min(2);
    Shimmer { id, elapsed, speed, bars }
}

/// Where bar `j` of shimmer `s` lies: the long bar, then the short one, then the long one again,
/// each a step behind the last.
pub fn shimmer_bar(s: &Shimmer, j: u32) -> (f32, f32) {
    query_bar((j % 2) as usize, s.elapsed * s.speed - j as f32 * QUERY_STEP)
}

/// A bar's ends are softened over this many points, inward, so a bar resting just off an edge
/// lights none of it.
pub const QUERY_SOFT: f32 = 3.0;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A bar tapers toward both ends, each over this share of its length, but no more than
/// `QUERY_TAPER_MAX` points and no less than `QUERY_SOFT`, so it blends into the line.
pub const QUERY_TAPER: f32 = 0.4;
pub const QUERY_TAPER_MAX: f32 = 60.0;

/// How much of `bar` lies at `s` points along an edge `span` points long, tapered at both ends.
pub fn bar_cover(bar: (f32, f32), s: f32, span: f32) -> f32 {
    let (a, b) = (bar.0 * span, bar.1 * span);
    let taper = (QUERY_TAPER * (b - a)).clamp(QUERY_SOFT, QUERY_TAPER_MAX);
    smoothstep(a, a + taper, s) * (1.0 - smoothstep(b - taper, b, s))
}

/// The lowest a shimmer's pulse takes its bars, as a share of full, and the pulses a second.
pub const SHIMMER_PULSE_LOW: f32 = 0.5;
pub const SHIMMER_PULSE_HZ: f32 = 1.4;

/// How bright shimmer `id`'s bars are `since` seconds after it began: a slow pulse between
/// `SHIMMER_PULSE_LOW` and full, each shimmer at a phase of its own.
pub fn shimmer_pulse(id: u32, since: f32) -> f32 {
    let wave = 0.5 + 0.5 * (std::f32::consts::TAU * (SHIMMER_PULSE_HZ * since + hash01(id.wrapping_add(3)))).cos();
    SHIMMER_PULSE_LOW + (1.0 - SHIMMER_PULSE_LOW) * wave
}

/// A key for an edge from the point it starts at (rounded to the nearest point) and its side's
/// bit, so the same edge keeps its key from frame to frame.
pub fn edge_key(x: f32, y: f32, side: u32) -> u32 {
    let ux = (x.max(0.0) + 0.5) as u32;
    let uy = (y.max(0.0) + 0.5) as u32;
    ux.wrapping_mul(73_856_093) ^ uy.wrapping_mul(19_349_663) ^ side.wrapping_mul(83_492_791)
}

/// Whether the edge with key `key` takes part in the shimmer with hash `id`.
pub fn edge_shows(key: u32, id: u32) -> bool {
    hash01(key ^ id.wrapping_mul(0x9E37_79B1)) < SHIMMER_EDGE_CHANCE
}

/// One coordinate of a CSS cubic Bezier from (0, 0) to (1, 1) with control values `a` and `b`,
/// at parameter `s`.
fn bezier_coord(a: f32, b: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
}

/// The CSS easing `cubic-bezier(c[0], c[1], c[2], c[3])` at progress `x`: the curve's x is
/// found by sixteen halvings, as the shader does, then its y read off. Its ends are exact, so a
/// bar at rest sits exactly off the edge.
pub fn cubic_bezier(c: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..16 {
        let mid = 0.5 * (lo + hi);
        if bezier_coord(c[0], c[2], mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier_coord(c[1], c[3], 0.5 * (lo + hi))
}

/// Where a bar of kind `kind` (0 the long bar, 1 the short) lies `x` seconds into its sweep at
/// speed 1: its start and end along the edge, as fractions from the end the bars come in at (0)
/// to the end they leave by (1). Before its sweep it waits behind the edge, after it past it.
pub fn query_bar(kind: usize, x: f32) -> (f32, f32) {
    let e = cubic_bezier(QUERY_EASE[kind], (x / QUERY_SWEEP).clamp(0.0, 1.0));
    let (from, to) = (QUERY_FROM[kind], QUERY_TO[kind]);
    let left = from[0] + (to[0] - from[0]) * e;
    let right = from[1] + (to[1] - from[1]) * e;
    (left, 1.0 - right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::BUILTIN;

    fn space() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "space").expect("space is built in")
    }

    /// The runs of `t` from 0 to `end` in steps of `dt` where `on` holds, as (start, end) pairs.
    fn runs(end: f32, dt: f32, on: impl Fn(f32) -> bool) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let mut began: Option<f32> = None;
        let steps = (end / dt) as u32;
        for i in 0..=steps {
            let t = i as f32 * dt;
            match (on(t), began) {
                (true, None) => began = Some(t),
                (false, Some(b)) => {
                    out.push((b, t));
                    began = None;
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn a_frozen_clock_shows_no_shooting_star_mid_flight_and_they_come_every_8_to_16_seconds() {
        assert!(!meteor_visible(0.0));

        // A shooting star every 8 to 16 seconds, each across in under a second.
        let dt = 0.01;
        let stars = runs(3600.0, dt, meteor_visible);
        assert!(stars.len() >= 225, "{}", stars.len());
        for w in stars.windows(2) {
            let gap = w[1].0 - w[0].0;
            assert!((8.0 - dt..=16.0 + dt).contains(&gap), "meteor gap {gap}");
        }
        for &(a, b) in &stars {
            assert!(b - a < 1.0, "a crossing of {} s", b - a);
        }
    }

    #[test]
    fn the_pane_edges_glow_narrower_and_bars_sweep_them_like_material_s_query_progress() {
        let wash = space().wash;
        let pair = |v: [f32; 2]| format!("float2({:?}, {:?})", v[0], v[1]);
        let quad = |v: [f32; 4]| format!("float4({:?}, {:?}, {:?}, {:?})", v[0], v[1], v[2], v[3]);
        for line in [
            "constant float EDGE_HALO = 0.75;".to_string(),
            "constant float EDGE_HALO_REACH = 2.0;".to_string(),
            "constant float EDGE_SHIMMER = 0.5;".to_string(),
            "constant float EDGE_CORE = 0.7;".to_string(),
            "constant float EDGE_CORE_WHITE = 0.15;".to_string(),
            "constant float EDGE_BAR_WHITE = 0.05;".to_string(),
            "constant float EDGE_BAR_GLOW = 1.5;".to_string(),
            "constant float EDGE_BAR_REACH = 0.5;".to_string(),
            "constant float EDGE_REACH = 12.0;".to_string(),
            format!("constant float QUERY_SWEEP = {QUERY_SWEEP:?};"),
            format!("constant float QUERY_STEP = {QUERY_STEP:?};"),
            format!("constant float2 QUERY_FROM[2] = {{{}, {}}};", pair(QUERY_FROM[0]), pair(QUERY_FROM[1])),
            format!("constant float2 QUERY_TO[2] = {{{}, {}}};", pair(QUERY_TO[0]), pair(QUERY_TO[1])),
            format!("constant float4 QUERY_EASE[2] = {{{}, {}}};", quad(QUERY_EASE[0]), quad(QUERY_EASE[1])),
            format!("constant float SHIMMER_SLOT = {SHIMMER_SLOT:?};"),
            format!("constant float SHIMMER_SLACK = {SHIMMER_SLACK:?};"),
            format!("constant float SHIMMER_OFFSET = {SHIMMER_OFFSET:?};"),
            format!("constant float SHIMMER_SPEED_MIN = {:?};", SHIMMER_SPEED[0]),
            format!("constant float SHIMMER_SPEED_MAX = {:?};", SHIMMER_SPEED[1]),
            format!("constant float SHIMMER_EDGE_CHANCE = {SHIMMER_EDGE_CHANCE:?};"),
            "constant uint SHIMMER_SALT = 0xC2B2AE35u;".to_string(),
        ] {
            assert!(wash.contains(&line), "wash.metal lacks {line}");
        }
        assert_eq!(SHIMMER_SALT, 0xC2B2_AE35);
        assert!(!wash.contains("EDGE_SHIMMER_SPACING") && !wash.contains("QUERY_CYCLE"), "the fixed cycle is gone");
    }

    #[test]
    fn a_passing_bar_glows_wider_and_brighter_and_the_glow_fades_out_inside_the_edge_search() {
        let wash = space().wash;
        // The halo grows with a bar's cover and is faded to nothing before EDGE_REACH, the
        // distance past which no edge is found, so it never ends in a step.
        assert!(wash.contains("exp(-dline / (EDGE_HALO_REACH * (1.0 + EDGE_BAR_REACH * cover))) * EDGE_HALO * (1.0 + EDGE_BAR_GLOW * cover)"));
        assert!(wash.contains("(1.0 - smoothstep(EDGE_REACH - 3.0, EDGE_REACH, dline))"));
        assert!(wash.contains("abs(d) < EDGE_REACH"), "the edge search reaches as far as the glow");
        assert!(wash.contains("if (dline < EDGE_REACH)"), "the bars are looked for as far as the glow");
        // A full bar's halo, as the shader works it out: nothing left at the search's edge, and
        // no sudden drop on the way there.
        let halo = |d: f32| (-d / (2.0 * 1.5)).exp() * 0.75 * 2.5 * (1.0 - smoothstep(9.0, 12.0, d));
        assert_eq!(halo(12.0), 0.0);
        for i in 60..120 {
            let d = i as f32 * 0.1;
            assert!(halo(d) - halo(d + 0.1) < 0.01, "a step at {d}");
        }
    }

    #[test]
    fn a_bar_follows_material_s_keyframes_and_rests_out_of_sight_before_and_after() {
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        // The long bar enters from just behind the edge's start and leaves past its end; the
        // short one from far behind to just past the end. Before and after, both are out of sight.
        assert!(near(query_bar(0, -1.0), (-0.35, 0.0)));
        assert!(near(query_bar(0, 0.0), (-0.35, 0.0)));
        assert!(near(query_bar(0, QUERY_SWEEP), (1.0, 1.9)), "{:?}", query_bar(0, QUERY_SWEEP));
        assert!(near(query_bar(0, 9.0), (1.0, 1.9)));
        assert!(near(query_bar(1, 0.0), (-2.0, 0.0)));
        assert!(near(query_bar(1, QUERY_SWEEP), (1.07, 1.08)));
        // Both sweep forward the whole way and pass the edge's middle.
        for kind in 0..2 {
            let mut last = query_bar(kind, 0.0);
            let mut covered_middle = false;
            for i in 1..=600 {
                let now = query_bar(kind, QUERY_SWEEP * i as f32 / 600.0);
                assert!(now.0 >= last.0 - 1e-5 && now.1 >= last.1 - 1e-5, "bar {kind} went back at step {i}");
                covered_middle |= now.0 <= 0.5 && now.1 >= 0.5;
                last = now;
            }
            assert!(covered_middle, "bar {kind} never crossed the middle");
        }
        // Material's pace: 60% of its 2.1 s cycle, and its second bar 1.15 s behind.
        assert!((QUERY_SWEEP - 0.6 * 2.1).abs() < 1e-6);
        assert_eq!(QUERY_STEP, 1.15);
    }

    /// Whether any bar of either shimmer playing at `t` lights any point of a 400 point edge.
    fn on_edge(t: f32) -> bool {
        shimmers_at(t).into_iter().flatten().any(|s| {
            (0..s.bars).any(|j| (0..=400).any(|x| bar_cover(shimmer_bar(&s, j), x as f32, 400.0) > 0.0))
        })
    }

    #[test]
    fn the_shimmer_still_playing_from_the_last_slot_is_looked_at_too() {
        // The first slot has no slot before it.
        let first = shimmers_at(0.0);
        assert!(first[0].is_none());
        assert_eq!(first[1], Some(shimmer_at(0.0)));
        // Later, the previous slot's shimmer is the one a slot earlier, with more time elapsed.
        let t = 100.0;
        let [prev, now] = shimmers_at(t);
        let (prev, now) = (prev.unwrap(), now.unwrap());
        assert_eq!(now, shimmer_at(t));
        assert_eq!(prev.id, shimmer_at(t - SHIMMER_SLOT).id);
        assert!((prev.elapsed - (shimmer_at(t - SHIMMER_SLOT).elapsed + SHIMMER_SLOT)).abs() < 1e-3);
    }

    #[test]
    fn a_bar_tapers_toward_both_ends_over_up_to_60_points() {
        let wash = space().wash;
        assert!(wash.contains(&format!("constant float QUERY_TAPER = {QUERY_TAPER:?};")));
        assert!(wash.contains(&format!("constant float QUERY_TAPER_MAX = {QUERY_TAPER_MAX:?};")));
        assert_eq!((QUERY_TAPER, QUERY_TAPER_MAX), (0.4, 60.0));
        // A long bar (240 points): 60 point tapers, full in between, rising the whole way in.
        let long = (0.2, 0.8);
        let (a, b) = (80.0, 320.0);
        assert_eq!(bar_cover(long, a + 60.0, 400.0), 1.0);
        assert_eq!(bar_cover(long, b - 60.0, 400.0), 1.0);
        assert_eq!(bar_cover(long, 200.0, 400.0), 1.0);
        assert!(bar_cover(long, a + 15.0, 400.0) < 0.2 && bar_cover(long, b - 15.0, 400.0) < 0.2);
        let mut last = 0.0;
        for i in 0..=60 {
            let c = bar_cover(long, a + i as f32, 400.0);
            assert!(c >= last, "the taper dips at {i}");
            last = c;
        }
        // A short bar (40 points) tapers over 40% of its length at each end.
        let short = (0.45, 0.55);
        assert_eq!(bar_cover(short, 180.0 + 16.0, 400.0), 1.0);
        assert!(bar_cover(short, 180.0 + 4.0, 400.0) < 0.2);
    }

    #[test]
    fn a_shimmer_pulses_between_half_and_full_brightness_each_at_its_own_phase() {
        let wash = space().wash;
        assert!(wash.contains(&format!("constant float SHIMMER_PULSE_LOW = {SHIMMER_PULSE_LOW:?};")));
        assert!(wash.contains(&format!("constant float SHIMMER_PULSE_HZ = {SHIMMER_PULSE_HZ:?};")));
        assert!(wash.contains("bar_cover(bar, along, span) * pulse"), "the pulse scales each bar");
        let period = 1.0 / SHIMMER_PULSE_HZ;
        let samples: Vec<f32> = (0..=100).map(|i| shimmer_pulse(7, period * i as f32 / 100.0)).collect();
        assert!(samples.iter().all(|&p| (SHIMMER_PULSE_LOW - 1e-4..=1.0 + 1e-4).contains(&p)));
        let (lo, hi) = (samples.iter().cloned().fold(1.0, f32::min), samples.iter().cloned().fold(0.0, f32::max));
        assert!(lo < SHIMMER_PULSE_LOW + 0.01 && hi > 0.99, "{lo} to {hi}");
        assert!((shimmer_pulse(7, 0.3) - shimmer_pulse(7, 0.3 + period)).abs() < 1e-3, "one period apart, the same");
        assert!((shimmer_pulse(7, 0.0) - shimmer_pulse(8, 0.0)).abs() > 1e-3, "two shimmers pulse out of step");
    }

    #[test]
    fn a_resting_bar_lights_no_part_of_the_edge_and_a_crossing_one_lights_its_middle() {
        for span in [40.0_f32, 400.0] {
            for x in 0..=span as u32 {
                let x = x as f32;
                assert_eq!(bar_cover(query_bar(0, 0.0), x, span), 0.0, "the long bar waiting, at {x}");
                assert_eq!(bar_cover(query_bar(1, 0.0), x, span), 0.0, "the short bar waiting, at {x}");
                assert_eq!(bar_cover(query_bar(0, QUERY_SWEEP), x, span), 0.0, "the long bar gone, at {x}");
                assert_eq!(bar_cover(query_bar(1, QUERY_SWEEP), x, span), 0.0, "the short bar gone, at {x}");
            }
        }
        assert_eq!(bar_cover((0.2, 0.8), 200.0, 400.0), 1.0);
        let wash = space().wash;
        assert!(wash.contains(&format!("constant float QUERY_SOFT = {QUERY_SOFT:?};")));
        assert!(wash.contains("float taper = clamp(QUERY_TAPER * (b - a), QUERY_SOFT, QUERY_TAPER_MAX);"));
        assert!(wash.contains("smoothstep(a, a + taper, s) * (1.0 - smoothstep(b - taper, b, s))"));
    }

    #[test]
    fn shimmers_come_every_1_5_to_5_5_seconds_with_one_to_three_bars_at_varied_speeds() {
        let dt = 0.01;
        let mut starts = Vec::new();
        let (mut counts, mut slowest, mut fastest) = ([0u32; 4], f32::MAX, 0.0_f32);
        let mut last_id = None;
        for i in 0..=(3600.0 / dt) as u32 {
            let t = i as f32 * dt;
            let s = shimmer_at(t);
            if Some(s.id) != last_id {
                last_id = Some(s.id);
                counts[s.bars as usize] += 1;
                slowest = slowest.min(s.speed);
                fastest = fastest.max(s.speed);
            }
            // A shimmer ends before the slot after next begins, so looking at this slot's and the
            // last one's is enough.
            if s.elapsed >= -dt {
                let end = ((s.bars - 1) as f32 * QUERY_STEP + QUERY_SWEEP) / s.speed;
                let began_at = (t + SHIMMER_OFFSET).rem_euclid(SHIMMER_SLOT) - s.elapsed;
                assert!(began_at + end <= 2.0 * SHIMMER_SLOT, "a shimmer at {t} runs too long");
            }
            if s.elapsed >= 0.0 && starts.last().is_none_or(|&(id, _)| id != s.id) {
                starts.push((s.id, t - s.elapsed));
            }
        }
        let gaps: Vec<f32> = starts.windows(2).map(|w| w[1].1 - w[0].1).collect();
        assert!(gaps.len() > 900, "{}", gaps.len());
        let (lo, hi) = (gaps.iter().cloned().fold(f32::MAX, f32::min), gaps.iter().cloned().fold(0.0, f32::max));
        assert!(lo >= 1.5 - 2.0 * dt && hi <= 5.5 + 2.0 * dt, "gaps from {lo} to {hi}");
        assert!(lo < 2.0 && hi > 5.0, "the gaps barely vary: {lo} to {hi}");
        assert_eq!(counts[0], 0);
        for n in 1..=3 {
            assert!(counts[n] > 60, "{n} bars: {} times", counts[n]);
        }
        assert!(slowest >= 0.8 && slowest < 0.85 && fastest <= 1.5 && fastest > 1.45, "{slowest} to {fastest}");
    }

    #[test]
    fn a_frozen_clock_shows_no_bar_and_bars_alternate_long_and_short() {
        assert!(!on_edge(0.0));
        assert!((0..3600).any(|i| on_edge(i as f32 * 0.1)));
        // Two shimmers can overlap now and then.
        let both = (0..36000).any(|i| {
            let [a, b] = shimmers_at(i as f32 * 0.1);
            let lit = |s: Option<Shimmer>| {
                s.is_some_and(|s| (0..s.bars).any(|j| (0..=40).any(|x| bar_cover(shimmer_bar(&s, j), x as f32 * 10.0, 400.0) > 0.0)))
            };
            lit(a) && lit(b)
        });
        assert!(both, "two shimmers never overlap");
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        let s = Shimmer { id: 7, elapsed: 0.0, speed: 1.0, bars: 3 };
        // At the start every bar waits behind the edge; the third is the long bar again.
        assert!(near(shimmer_bar(&s, 0), query_bar(0, 0.0)));
        assert!(near(shimmer_bar(&s, 1), query_bar(1, -QUERY_STEP)));
        let late = Shimmer { elapsed: 2.0 * QUERY_STEP + 0.5, ..s };
        assert!(near(shimmer_bar(&late, 2), query_bar(0, 0.5)));
        // A faster shimmer gets as far in less time.
        let fast = Shimmer { elapsed: 0.25, speed: 2.0, ..s };
        assert!(near(shimmer_bar(&fast, 0), query_bar(0, 0.5)));
    }

    #[test]
    fn each_shimmer_picks_its_own_edges_and_an_edge_s_pick_is_stable() {
        let keys: Vec<u32> = (0..40).map(|i| edge_key(i as f32 * 37.0, (i % 5) as f32 * 120.0, 1 << (i % 4))).collect();
        // Within one shimmer some edges show and some do not.
        let picked = keys.iter().filter(|&&k| edge_shows(k, 11)).count();
        assert!(picked > 8 && picked < 32, "{picked} of 40");
        // Across shimmers one edge shows about 55% of the time, not always.
        let shown = (0..1000u32).filter(|&id| edge_shows(keys[3], id)).count();
        assert!((450..650).contains(&shown), "{shown} of 1000");
        // The same edge, a fraction of a point off, is the same edge.
        assert_eq!(edge_key(100.2, 40.4, 2), edge_key(99.8, 39.6, 2));
        assert_ne!(edge_key(100.0, 40.0, 2), edge_key(100.0, 40.0, 4));
    }

    #[test]
    fn a_cubic_bezier_runs_from_0_to_1_and_matches_css_at_its_ends() {
        for c in QUERY_EASE {
            assert_eq!(cubic_bezier(c, 0.0), 0.0);
            assert_eq!(cubic_bezier(c, 1.0), 1.0);
        }
        // The linear curve is the identity.
        for x in [0.1_f32, 0.25, 0.5, 0.9] {
            assert!((cubic_bezier([0.0, 0.0, 1.0, 1.0], x) - x).abs() < 1e-3);
        }
        // CSS's ease-out (0, 0, 0.58, 1) is well ahead at the halfway point.
        assert!(cubic_bezier([0.0, 0.0, 0.58, 1.0], 0.5) > 0.6);
    }

    #[test]
    fn the_wash_carries_the_schedule_s_constants_and_names_this_mirror() {
        let wash = space().wash;
        assert!(wash.contains("effects/space.rs"), "the shader names its mirror");
        assert!(!wash.to_lowercase().contains("aurora_"), "the aurora is gone");
        for line in [
            "constant float METEOR_CYCLE = 12.0;",
            "constant float METEOR_SLACK = 4.0;",
            "constant float METEOR_FLIGHT = 0.8;",
            "constant float METEOR_OFFSET = 6.0;",
            "constant uint METEOR_SALT = 0x9E3779B9u;",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
    }

    #[test]
    fn the_menus_edges_glow_and_shimmer_with_the_panes_own_code() {
        let wash = space().wash;
        let start = wash.find("fragment float4 arkos_overlay(").expect("space draws the menus' edges");
        let overlay = &wash[start..];
        // The nearest edge, its bars and its glow come from the functions the panes use.
        for f in ["nearest_edge(", "bar_cover_at(", "edge_glow("] {
            assert!(overlay.contains(f), "the menu stage does not call {f}");
            assert!(wash[..start].matches(f).count() >= 2, "{f} is not shared with the wash");
        }
        assert!(overlay.contains("ARKOS_MENU_BLOCK") || overlay.contains("blk.value"), "the edges fade with the menu");
    }

    #[test]
    fn emoji_are_shown_on_the_holo_display_with_the_text_s_own_values_and_are_still() {
        let glyphs = space().glyphs.unwrap();
        let start = glyphs.find("vertex HoloEmojiVertexOutput arkos_emoji_vertex(").expect("space draws its emoji");
        let emoji = &glyphs[start..];
        assert!(emoji.contains("fragment float4 arkos_emoji_fragment("));
        // The same tint, keep, scanlines, brightness and glow as the letters, by name, so the two
        // never drift apart; the halo reaches as far.
        for name in ["TINT", "KEEP", "SCANLINES", "BRIGHTNESS", "GLOW", "REACH", "HOLO_CYAN"] {
            assert!(emoji.contains(name), "the emoji stages do not use {name}");
        }
        // Their own colours, tinted: the sample's colour is the starting point, not the text's.
        assert!(emoji.contains("float3 col = core.rgb;"));
        // Still, as the text is: no clock.
        assert!(!emoji.contains("[[buffer(5)]]"));
        // Brightness scales the colour, never the coverage.
        assert!(emoji.contains("col * light * a"));
    }

    #[test]
    fn the_holo_text_is_measured_in_points_is_still_and_keeps_dark_text_readable() {
        let glyphs = space().glyphs.expect("space draws its own glyphs");
        assert!(glyphs.contains("arkos_glyph_vertex(") && glyphs.contains("arkos_glyph_fragment("));
        // Points through the window's scale, and no clock: the text never moves.
        assert!(glyphs.contains("ARKOS_SCALE"));
        assert!(!glyphs.contains("[[buffer(5)]]"), "the holo text reads no clock");
        // Cyrus's values, verbatim.
        for line in [
            "constant float BRIGHTNESS = 0.80;",
            "constant float TINT = 0.25;",
            "constant float KEEP = 0.15;",
            "constant float SCANLINES = 0.08;",
            "constant float GLOW = 1.40;",
            "constant float REACH = 5.5;",
        ] {
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        // Dark text (a light theme with space) tints toward deep teal, undimmed and unglowing.
        assert!(glyphs.contains("bool dark = luminance < 0.5;"));
        assert!(glyphs.contains("float3(0.0, 0.35, 0.45)"));
        // The stroke's core keeps at least 55% of its coverage.
        assert!(glyphs.contains("core * 0.55"));
    }
}
