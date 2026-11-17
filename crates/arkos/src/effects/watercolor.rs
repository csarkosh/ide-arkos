//! The watercolor wash's arithmetic the tests check against its shader text: Pane 0's gallery
//! schedule, mirrored from `gallery_at` in `effects/watercolor/wash.metal`.

#[cfg(test)]
mod tests {
    const GALLERY_HOLD: f32 = 50.0;
    const GALLERY_BLOOM: f32 = 6.0;

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
        let (shown, next, mid) = gallery_at(53.0, n);
        assert_eq!((shown, next), (0, 1));
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
        assert_eq!(gallery_at(56.0, n), (1, 2, 0.0), "the next one shows, held");
        // Through all eight in order, and the first again after the last.
        let order: Vec<usize> = (0..9).map(|i| gallery_at(i as f32 * 56.0 + 1.0, n).0).collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!(gallery_at(7.0 * 56.0 + 53.0, n).1, 0, "the last blooms into the first");
        // Under Reduce Motion the clock holds at zero: the first painting, still.
        assert_eq!(gallery_at(0.0, n), (0, 1, 0.0));
    }

    #[test]
    fn the_wash_s_gallery_is_the_one_checked_here() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        for line in [
            "constant float GALLERY_HOLD = 50.0;",
            "constant float GALLERY_BLOOM = 6.0;",
            "    float period = GALLERY_HOLD + GALLERY_BLOOM;",
            "    float cycle = floor(max(t, 0.0) / period);",
            "    float shown = fmod(cycle, float(n));",
            "    float next = fmod(cycle + 1.0, float(n));",
            "    float bloom = saturate((into - GALLERY_HOLD) / GALLERY_BLOOM);",
            "    return float3(shown, next, bloom * bloom * (3.0 - 2.0 * bloom));",
            // After the paper is kept readable, so the paintings keep their own darks.
            "    // The gallery, after the paper is kept readable: no text is over the space it hangs in.",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
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

    /// Painting i's canvas on the easel, as the wash's `canvas_rect` places it in a sidebar foot
    /// `foot` (x, y, w, h): its origin and size in points.
    fn canvas_rect(foot: [f32; 4], size: (f32, f32)) -> [f32; 4] {
        let w = wash();
        let (sw, sh) = (foot[2] - 8.0, (foot[3] - 6.0).min((foot[2] - 8.0) * constant(w, "STUDIO_TALL")));
        let (ox, oy) = (foot[0] + 4.0, foot[1] + foot[3] - 4.0);
        let (bw, bh) = (sw * constant(w, "CANVAS_W"), sh * constant(w, "CANVAS_H"));
        let k = (bw / size.0).min(bh / size.1);
        let (cw, ch) = (size.0 * k, size.1 * k);
        let (fx, fy) = (ox + constant(w, "CANVAS_X") * sw, oy - constant(w, "CANVAS_FOOT") * sh);
        [fx - cw * 0.5, fy - ch, cw, ch]
    }

    #[test]
    fn the_painting_is_the_studio_s_centrepiece_on_its_easel_above_the_floor() {
        let w = wash();
        let effect = crate::effects::manifest::load("watercolor", std::path::Path::new("/nonexistent")).unwrap();
        let sizes: Vec<(f32, f32)> = effect.images.iter().map(|b| crate::effects::images::dimensions(b).map(|(a, b)| (a as f32, b as f32)).unwrap()).collect();
        for foot in [[0.0f32, 300.0, 240.0, 500.0], [8.0, 420.0, 200.0, 180.0], [0.0, 100.0, 320.0, 700.0]] {
            let (sw, sh) = (foot[2] - 8.0, (foot[3] - 6.0).min((foot[2] - 8.0) * constant(w, "STUDIO_TALL")));
            let floor = foot[1] + foot[3] - 4.0 - constant(w, "FLOOR_Y") * sh;
            for &size in &sizes {
                let r = canvas_rect(foot, size);
                assert!(r[0] >= foot[0] && r[0] + r[2] <= foot[0] + foot[2] && r[1] >= foot[1], "{foot:?} {size:?}: {r:?} outside the space");
                assert!(r[1] + r[3] < floor, "{foot:?} {size:?}: the canvas reaches the floor");
                // The centrepiece: wide or tall, it fills its share of the scene one way.
                assert!(r[2] >= 0.99 * sw * constant(w, "CANVAS_W") || r[3] >= 0.99 * sh * constant(w, "CANVAS_H"), "{foot:?} {size:?}: {r:?}");
            }
        }
        assert!(w.contains("float3 tone = ") || w.contains("float3 tone)"), "the painting is toned toward the theme");
        assert!(constant(w, "STUDIO_TONE") > 0.0 && constant(w, "STUDIO_TONE") <= 0.35, "toned, not repainted");
    }

}
