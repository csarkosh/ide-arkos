//! The terminal's paint-in, watercolor's counterpart to the decode: a cell the program writes
//! is written in, as by a pen. The CPU only stamps: each row's cells changed at one instant share a
//! stamp, the instant on the glyph clock less the stagger to the row's first changed cell, and
//! the glyph shader paints from it (`watercolor/glyphs.metal`). Pure rules, no drawing.
use std::time::{Duration, Instant};

/// How long the pen takes to write one cell's glyph, in seconds.
pub const PAINT_DUR: f32 = 0.12;
/// How long freshly written ink takes to dry, in seconds.
pub const PAINT_DRY: f32 = 0.8;
/// How much later each cell to the right starts, in seconds a point: 40 ms a cell of the
/// default terminal's 9.6 point wide cells, a quick hand rather than a printer.
pub const PAINT_STAGGER_PER_PT: f32 = 0.040 / 9.6;
/// How long past its drying the scene is still presented, so the last frame shows the ink dry.
pub const PAINT_MARGIN: Duration = Duration::from_millis(50);

/// The lines that tell a glyph shader the paint's timing: `ARKOS_PAINT_DUR`, `ARKOS_PAINT_DRY`
/// and `ARKOS_PAINT_STAGGER` (seconds a point).
pub fn paint_lines() -> String {
    format!(
        "constant float ARKOS_PAINT_DUR = {PAINT_DUR};\nconstant float ARKOS_PAINT_DRY = {PAINT_DRY};\nconstant float ARKOS_PAINT_STAGGER = {PAINT_STAGGER_PER_PT:.8};"
    )
}

/// One row's stamps and when its ink is dry. `changed` is each cell's change instant (None for a
/// cell that never changed or settled at once), `x` a column's left edge in points from the
/// window's left, `clock` the glyph clock at `now`. Cells changed at one instant share a stamp:
/// the instant on the glyph clock less the stagger up to the first of them, so the shader, which
/// adds each glyph's own x times the stagger, starts that first cell at the instant and each
/// one after it a little later. A cell whose ink is already dry gets 0, which the shader draws
/// as ever, as does an unchanged one; a real stamp that would be 0 is nudged off it. The dry
/// instant is that of the latest-drying cell still wet, None when none is.
pub fn row_stamps(changed: &[Option<Instant>], x: impl Fn(usize) -> f32, clock: f32, now: Instant) -> (Vec<f32>, Option<Instant>) {
    let mut stamps = vec![0.0; changed.len()];
    let mut dry: Option<Instant> = None;
    let mut done = vec![false; changed.len()];
    for first in 0..changed.len() {
        let Some(at) = changed[first] else { continue };
        if done[first] {
            continue;
        }
        let same: Vec<usize> = (first..changed.len()).filter(|&c| changed[c] == Some(at)).collect();
        let last = *same.last().unwrap_or(&first);
        let wet = PAINT_DUR + PAINT_DRY + (x(last) - x(first)) * PAINT_STAGGER_PER_PT;
        let ends = at + Duration::from_secs_f32(wet);
        let since = now.saturating_duration_since(at).as_secs_f32();
        let stamp = clock - since - x(first) * PAINT_STAGGER_PER_PT;
        let stamp = if stamp == 0.0 { f32::MIN_POSITIVE } else { stamp };
        for c in same {
            done[c] = true;
            if ends > now {
                stamps[c] = stamp;
            }
        }
        if ends > now {
            dry = dry.max(Some(ends));
        }
    }
    (stamps, dry)
}

/// The glyph clock the terminal stamps with, under an effect that paints the terminal: None
/// without one, with Reduce Motion or while the window is `paused`, when the clock stands still
/// and ink stamped on it would never dry.
pub fn paint_clock(style_paint: bool, reduce: bool, paused: bool, clock: f32) -> Option<f32> {
    (style_paint && !reduce && !paused).then_some(clock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(col: usize) -> f32 {
        100.0 + col as f32 * 9.6
    }

    #[test]
    fn cells_changed_together_share_a_stamp_that_starts_the_first_at_its_instant() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(100);
        let changed = [None, Some(t0), Some(t0), Some(now), None];
        let (stamps, dry) = row_stamps(&changed, x, 50.0, now);
        assert_eq!(stamps[0], 0.0);
        assert_eq!(stamps[4], 0.0);
        assert_eq!(stamps[1], stamps[2]);
        // The shader starts a glyph at its stamp plus its x times the stagger.
        let start = |c: usize| stamps[c] + x(c) * PAINT_STAGGER_PER_PT;
        assert!((start(1) - 49.9).abs() < 1e-4, "{}", start(1));
        assert!((start(2) - start(1) - 0.040).abs() < 1e-4);
        assert!((start(3) - 50.0).abs() < 1e-4, "a cell changed now starts now, wherever it is");
        let wet = Duration::from_secs_f32(PAINT_DUR + PAINT_DRY);
        assert_eq!(dry, Some(now + wet));
    }

    #[test]
    fn dry_ink_draws_as_ever_and_the_row_is_dry() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(2);
        let (stamps, dry) = row_stamps(&[Some(t0), Some(t0), None], x, 50.0, now);
        assert_eq!(stamps, vec![0.0, 0.0, 0.0]);
        assert_eq!(dry, None);
        assert_eq!(row_stamps(&[], x, 50.0, now), (vec![], None));
    }

    #[test]
    fn a_long_run_stays_wet_until_its_last_cell_dries() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(1100);
        let changed = vec![Some(t0); 80];
        let (stamps, dry) = row_stamps(&changed, x, 50.0, now);
        assert!(stamps.iter().all(|&s| s != 0.0), "the run's end is still wet");
        assert!(dry.is_some_and(|d| d > now));
    }

    #[test]
    fn a_stamp_never_lands_on_the_dry_mark() {
        let now = Instant::now();
        let (stamps, _) = row_stamps(&[Some(now)], |_| 0.0, 0.0, now);
        assert_ne!(stamps[0], 0.0);
    }

    #[test]
    fn the_terminal_paints_only_under_a_painting_effect_with_the_clock_running() {
        assert_eq!(paint_clock(true, false, false, 3.5), Some(3.5));
        assert_eq!(paint_clock(false, false, false, 3.5), None);
        assert_eq!(paint_clock(true, true, false, 3.5), None);
        assert_eq!(paint_clock(true, false, true, 3.5), None);
    }
}
