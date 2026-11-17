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
}
