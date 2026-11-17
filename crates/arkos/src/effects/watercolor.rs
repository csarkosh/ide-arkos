//! The watercolor wash's arithmetic the tests check against its shader text: Pane 0's gallery
//! schedule, mirrored from `gallery_at` in `effects/watercolor/wash.metal`.

#[cfg(test)]
mod tests {
    const GALLERY_HOLD: f32 = 50.0;
    const GALLERY_BLOOM: f32 = 12.0;

    /// Which painting shows at `t`, the next, and how far the bloom into it has come, as the
    /// wash's `gallery_at` works it out.
    fn gallery_at(t: f32, n: usize) -> (usize, usize, f32) {
        let period = GALLERY_HOLD + GALLERY_BLOOM;
        let t = t.max(0.0);
        let cycle = (t / period).floor();
        let into = t - cycle * period;
        let bloom = ((into - GALLERY_HOLD) / GALLERY_BLOOM).clamp(0.0, 1.0);
        ((cycle as usize) % n, (cycle as usize + 1) % n, bloom * bloom * (3.0 - 2.0 * bloom))
    }

    #[test]
    fn the_gallery_holds_each_painting_then_blooms_into_the_next_in_order_and_starts_again() {
        let n = 8;
        // Held whole for its fifty seconds.
        for t in [0.0, 10.0, 49.9] {
            assert_eq!(gallery_at(t, n), (0, 1, 0.0), "{t}");
        }
        // The bloom: under way, then the next painting whole.
        let (shown, next, mid) = gallery_at(56.0, n);
        assert_eq!((shown, next), (0, 1));
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
        assert_eq!(gallery_at(62.0, n), (1, 2, 0.0), "the next one shows, held");
        // Through all eight in order, and the first again after the last.
        let order: Vec<usize> = (0..9).map(|i| gallery_at(i as f32 * 62.0 + 1.0, n).0).collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!(gallery_at(7.0 * 62.0 + 56.0, n).1, 0, "the last blooms into the first");
        // Under Reduce Motion the clock holds at zero: the first painting, still.
        assert_eq!(gallery_at(0.0, n), (0, 1, 0.0));
    }

    #[test]
    fn the_wash_s_gallery_is_the_one_checked_here() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        for line in [
            "constant float GALLERY_HOLD = 50.0;",
            "constant float GALLERY_BLOOM = 12.0;",
            "    float period = GALLERY_HOLD + GALLERY_BLOOM;",
            "    float cycle = floor(max(t, 0.0) / period);",
            "    float shown = fmod(cycle, float(n));",
            "    float next = fmod(cycle + 1.0, float(n));",
            "    float bloom = saturate((into - GALLERY_HOLD) / GALLERY_BLOOM);",
            "    return float3(shown, next, bloom * bloom * (3.0 - 2.0 * bloom));",
            // A wide soft blend, each point over half the bloom, the paint thinning at its middle,
            // and no darker front.
            "    return smoothstep(reach * 0.5, reach * 0.5 + 0.5, bloom);",
            "    float thin = 1.0 - GALLERY_THIN * sin(3.14159 * at.z);",
            // After the paper is kept readable, so the paintings keep their own darks.
            "    // The gallery, after the paper is kept readable: no text is over the space it hangs in.",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        assert!(!wash.contains("float front = "), "no wet front darkening the bloom");
        let readable = wash.find("    col = keep_readable(u, col);").unwrap();
        assert!(wash.find("if (ARKOS_IMAGE_COUNT > 0) {").unwrap() > readable);
    }

    /// A shader constant's value, read from the wash's text.
    fn constant(wash: &str, name: &str) -> f32 {
        let line = wash.lines().find(|l| l.starts_with(&format!("constant float {name} = "))).unwrap_or_else(|| panic!("no {name}"));
        line.rsplit("= ").next().unwrap().trim_end_matches(';').parse().unwrap()
    }

    fn wash() -> &'static str {
        crate::effects::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash
    }

    #[test]
    fn the_studio_moves_softly_every_motion_slow_and_small() {
        // Cyrus, 2026-11-17: "soft movements that adds liveliness to the scene without being
        // distracting": nothing in the studio cycles faster than every five seconds.
        let w = wash();
        for name in ["LAMP_SWAY_S", "DRAPE_SWAY_S", "LEAF_SWAY_S", "STEAM_RISE_S", "BEAM_BREATH_S", "CLOUD_PASS_S"] {
            assert!(constant(w, name) >= 5.0, "{name} is {}", constant(w, name));
        }
        assert!(constant(w, "LAMP_SWAY") <= 0.05, "the lamp swings a few degrees at most");
        for motion in ["t * tau / LAMP_SWAY_S", "t * tau / DRAPE_SWAY_S", "t * tau / LEAF_SWAY_S", "t * tau / STEAM_RISE_S", "t * tau / BEAM_BREATH_S", "t / CLOUD_PASS_S"] {
            assert!(w.contains(motion), "the studio's {motion} is not used");
        }
    }

    /// The easel's backboard, as the wash's `board_rect` places it in a sidebar foot `foot`
    /// (x, y, w, h): its origin and size in points.
    fn board_rect(foot: [f32; 4]) -> [f32; 4] {
        let w = wash();
        let (sw, sh) = (foot[2] - 8.0, (foot[3] - 6.0).min((foot[2] - 8.0) * constant(w, "STUDIO_TALL")));
        let (ox, oy) = (foot[0] + 4.0, foot[1] + foot[3] - 4.0);
        let (bw, bh) = (sw * constant(w, "CANVAS_W"), sh * constant(w, "CANVAS_H"));
        let (fx, fy) = (ox + constant(w, "CANVAS_X") * sw, oy - constant(w, "CANVAS_FOOT") * sh);
        [fx - bw * 0.5, fy - bh, bw, bh]
    }

    /// Painting i on the board, as the wash's `canvas_rect` places it.
    fn canvas_rect(foot: [f32; 4], size: (f32, f32)) -> [f32; 4] {
        let pad = constant(wash(), "BOARD_PAD");
        let b = board_rect(foot);
        let (bw, bh) = (b[2] - 2.0 * pad, b[3] - 2.0 * pad);
        let k = (bw / size.0).min(bh / size.1);
        let (cw, ch) = (size.0 * k, size.1 * k);
        [b[0] + (b[2] - cw) * 0.5, b[1] + (b[3] - ch) * 0.5, cw, ch]
    }

    #[test]
    fn the_painting_is_the_studio_s_centrepiece_on_its_easel_above_the_floor() {
        let w = wash();
        let effect = crate::effects::manifest::load("watercolor", std::path::Path::new("/nonexistent")).unwrap();
        let sizes: Vec<(f32, f32)> = effect.images.iter().map(|b| crate::effects::images::dimensions(b).map(|(a, b)| (a as f32, b as f32)).unwrap()).collect();
        for foot in [[0.0f32, 300.0, 240.0, 500.0], [8.0, 420.0, 200.0, 180.0], [0.0, 100.0, 320.0, 700.0]] {
            let (sw, sh) = (foot[2] - 8.0, (foot[3] - 6.0).min((foot[2] - 8.0) * constant(w, "STUDIO_TALL")));
            let floor = foot[1] + foot[3] - 4.0 - constant(w, "FLOOR_Y") * sh;
            let b = board_rect(foot);
            assert!(b[0] >= foot[0] && b[0] + b[2] <= foot[0] + foot[2] && b[1] >= foot[1], "{foot:?}: the board {b:?} outside the space");
            assert!(b[1] + b[3] < floor, "{foot:?}: the board reaches the floor");
            let pad = constant(w, "BOARD_PAD");
            for &size in &sizes {
                let r = canvas_rect(foot, size);
                // On the board, a margin of it all round.
                assert!(r[0] >= b[0] + pad - 0.01 && r[0] + r[2] <= b[0] + b[2] - pad + 0.01 && r[1] >= b[1] + pad - 0.01 && r[1] + r[3] <= b[1] + b[3] - pad + 0.01, "{foot:?} {size:?}: {r:?} off the board {b:?}");
                // The centrepiece: wide or tall, it fills the board one way.
                assert!(r[2] >= 0.99 * (b[2] - 2.0 * pad) || r[3] >= 0.99 * (b[3] - 2.0 * pad), "{foot:?} {size:?}: {r:?}");
            }
            let _ = (sw, sh);
        }
        assert!(w.contains("float3 tone = ") || w.contains("float3 tone)"), "the painting is toned toward the theme");
        assert!(constant(w, "STUDIO_TONE") > 0.0 && constant(w, "STUDIO_TONE") <= 0.35, "toned, not repainted");
        // Its edges near straight (Cyrus, 2026-11-17: "Make the painting's edges straighter").
        assert!(constant(w, "PAINTING_WAVER") <= 1.0, "a sheet cut by hand, not torn");
    }


    /// The cat as the wash's `cat_at` places it at t: across the scene, away (0), walking (1) or
    /// sitting (2), which way it faces.
    fn cat_at(t: f32) -> (f32, u8, f32) {
        let w = wash();
        let (cycle, speed, sit) = (constant(w, "CAT_CYCLE"), constant(w, "CAT_SPEED"), constant(w, "CAT_SIT"));
        let (table, easel, plant) = (constant(w, "CAT_SPOT_TABLE"), constant(w, "CAT_SPOT_EASEL"), constant(w, "CAT_SPOT_PLANT"));
        let hash1 = |n: f32| ((n.sin() * 43758.547).fract() + 1.0).fract();
        let t = t.max(0.0);
        let k = (t / cycle).floor();
        let tau = t - k * cycle;
        let dir = if hash1(k * 3.1 + 0.7) < 0.5 { 1.0 } else { -1.0 };
        let pick = (hash1(k * 5.7 + 1.3) * 3.0).floor();
        let (mut a, mut b) = (if pick == 2.0 { easel } else { table }, if pick == 0.0 { easel } else { plant });
        if dir < 0.0 {
            std::mem::swap(&mut a, &mut b);
        }
        let (x0, x3) = if dir > 0.0 { (-0.12, 1.12) } else { (1.12, -0.12) };
        let e1 = (a - x0).abs() / speed;
        let e2 = e1 + sit;
        let e3 = e2 + (b - a).abs() / speed;
        let e4 = e3 + sit;
        let e5 = e4 + (x3 - b).abs() / speed;
        if tau < e1 {
            (x0 + dir * tau * speed, 1, dir)
        } else if tau < e2 {
            (a, 2, dir)
        } else if tau < e3 {
            (a + dir * (tau - e2) * speed, 1, dir)
        } else if tau < e4 {
            (b, 2, dir)
        } else if tau < e5 {
            (b + dir * (tau - e4) * speed, 1, dir)
        } else {
            (x3, 0, dir)
        }
    }

    #[test]
    fn the_cat_strolls_in_sits_by_two_things_in_turn_and_strolls_off_then_is_away_a_while() {
        let w = wash();
        let (cycle, speed, sit) = (constant(w, "CAT_CYCLE"), constant(w, "CAT_SPEED"), constant(w, "CAT_SIT"));
        let spots = [constant(w, "CAT_SPOT_TABLE"), constant(w, "CAT_SPOT_EASEL"), constant(w, "CAT_SPOT_PLANT")];
        // Soft: a slow stroll, a slow stride.
        assert!(speed <= 0.04 && constant(w, "CAT_STRIDE_S") >= 0.6);
        assert!(cycle >= 120.0, "a visit now and then, not always there");
        // Under Reduce Motion (the clock at zero) it has not come in.
        let (x, state, _) = cat_at(0.0);
        assert!(state == 1 && !(0.0..=1.0).contains(&x), "off the scene at zero: {x}");
        for visit in 0..12 {
            let start = visit as f32 * cycle;
            let (mut sat_at, mut away, mut last) = (Vec::new(), 0.0f32, None::<(f32, u8)>);
            let mut t = start;
            while t < start + cycle {
                let (x, state, _) = cat_at(t);
                if let Some((lx, ls)) = last {
                    // Never a jump: it walks from place to place.
                    assert!((x - lx).abs() <= speed * 0.1 + 1e-4 || (ls == 1 && state != 1) || (ls != 1 && state == 1) || state == 0, "visit {visit} at {t}: {lx} to {x}");
                }
                match state {
                    2 => {
                        if sat_at.last() != Some(&x) {
                            sat_at.push(x);
                        }
                    }
                    0 => away += 0.1,
                    _ => {}
                }
                last = Some((x, state));
                t += 0.1;
            }
            assert_eq!(sat_at.len(), 2, "visit {visit} sits by two things: {sat_at:?}");
            assert!(sat_at.iter().all(|x| spots.contains(x)) && sat_at[0] != sat_at[1], "visit {visit}: {sat_at:?}");
            assert!(away >= 0.25 * cycle, "visit {visit}: away only {away} s of {cycle}");
            let (x, state, _) = cat_at(start + cycle - 0.05);
            assert!(state == 0 && !(0.0..=1.0).contains(&x), "visit {visit} ends off the scene");
            let _ = sit;
        }
    }


    /// Bird i as the wash's `bird_at` places it: across the glass, up, its depth.
    fn bird_at(i: usize, t: f32) -> (f32, f32, f32) {
        let w = wash();
        let hash1 = |n: f32| ((n.sin() * 43758.547).fract() + 1.0).fract();
        let fi = i as f32;
        let n = constant_int(w, "BIRDS") as f32;
        let depth = ((fi + 0.5 + (hash1(fi * 7.3 + 0.4) - 0.5) * 0.6) / n).clamp(0.0, 1.0);
        let speed = constant(w, "BIRD_FAR_SPEED") + (constant(w, "BIRD_NEAR_SPEED") - constant(w, "BIRD_FAR_SPEED")) * depth;
        let cross = 1.3 / speed;
        let period = cross + 25.0 + 45.0 * hash1(fi * 3.9 + 2.0);
        let dir = if hash1(fi * 5.1 + 1.0) < 0.5 { 1.0 } else { -1.0 };
        let u = (t.max(0.0) + hash1(fi * 9.7) * period) % period;
        let mut x = if u < cross { -0.15 + u * speed } else { 2.0 };
        if dir < 0.0 && x < 1.5 {
            x = 1.0 - x;
        }
        let y = 0.55 + 0.38 * hash1(fi * 2.3 + 5.0) + 0.025 * (t * 0.6 + fi * 1.7).sin();
        (x, y, depth)
    }

    #[test]
    fn the_birds_fly_at_their_own_distances_and_paces_each_on_its_own() {
        let w = wash();
        let n = constant_int(w, "BIRDS");
        assert!(n >= 4, "a few birds");
        assert!(constant(w, "BIRD_FAR_SPEED") < constant(w, "BIRD_NEAR_SPEED") && constant(w, "BIRD_FAR_SPAN") < constant(w, "BIRD_NEAR_SPAN"), "nearer is faster and larger");
        // Each its own depth, so its own pace.
        let depths: Vec<f32> = (0..n).map(|i| bird_at(i, 0.0).2).collect();
        for (a, da) in depths.iter().enumerate() {
            for db in &depths[a + 1..] {
                assert!((da - db).abs() > 0.02, "two birds share a depth: {depths:?}");
            }
        }
        assert!(depths.iter().cloned().fold(f32::MAX, f32::min) < 0.45 && depths.iter().cloned().fold(0.0, f32::max) > 0.6, "far ones and near ones: {depths:?}");
        // Over ten minutes every bird crosses the glass and is away between, in the sky above the
        // rooftops, and they are not all over the window together.
        let (mut seen, mut away) = (vec![false; n], vec![false; n]);
        let mut t = 0.0f32;
        let mut most_at_once = 0;
        while t < 600.0 {
            let mut now = 0;
            for i in 0..n {
                let (x, y, _) = bird_at(i, t);
                if (0.0..=1.0).contains(&x) {
                    seen[i] = true;
                    now += 1;
                    assert!((0.5..=0.96).contains(&y), "bird {i} out of the sky at {y}");
                } else {
                    away[i] = true;
                }
            }
            most_at_once = most_at_once.max(now);
            t += 0.25;
        }
        assert!(seen.iter().all(|s| *s) && away.iter().all(|a| *a), "every bird crosses and leaves: {seen:?} {away:?}");
        assert!(most_at_once < n, "never the whole flock at once");
        assert!(w.contains("bird_at(b, t)"), "the window draws them");
    }

    fn constant_int(wash: &str, name: &str) -> usize {
        let line = wash.lines().find(|l| l.starts_with(&format!("constant int {name} = "))).unwrap();
        line.rsplit("= ").next().unwrap().trim_end_matches(';').parse().unwrap()
    }

}
