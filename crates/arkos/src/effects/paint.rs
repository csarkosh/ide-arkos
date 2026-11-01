//! The terminal's paint-in, watercolor's counterpart to the decode: a cell the program writes
//! is written in, as by a pen. The CPU only stamps: each changed cell gets its own start on the
//! glyph clock, the cells changed at one instant in a row starting left to right in one hand, which
//! the stamp also names, and the glyph shader paints from it (`watercolor/glyphs.metal`). Pure
//! rules, no drawing.
use super::hash::hash01;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the pen takes to write one cell's glyph, in seconds, for a hand of glyph time 1.
pub const PAINT_DUR: f32 = 0.04;
/// How long freshly written ink takes to dry, in seconds.
pub const PAINT_DRY: f32 = 0.8;
/// How much later each cell to the right starts, in seconds a point, for a hand of pace 1: 11 ms
/// a cell of the default terminal's 9.6 point wide cells, on average along a line short enough
/// to be written within `PAINT_LINE`.
pub const PAINT_STAGGER_PER_PT: f32 = 0.011 / 9.6;
/// How long a run of new text takes to be written however long it is, in seconds: Material 3's
/// medium4. A longer one starts its cells closer together, so its last starts `PAINT_DUR` before
/// this. Along the line the pen follows Material 3's standard decelerate curve, as text enters
/// (`unease`), and so does each glyph's stroke in the shader, so text shows at once and settles.
pub const PAINT_LINE: f32 = 0.4;
/// How many rows the pen writes at a time, as the decode scrambles only its newest few
/// (`decode::DECODE_ROWS`): the rows changed most recently, the lowest on screen first among rows
/// changed at one instant. Older text shows as written, so a screenful of output does not all
/// move at once.
pub const PAINT_ROWS: usize = 4;
/// How long past its drying the scene is still presented, so the last frame shows the ink dry.
pub const PAINT_MARGIN: Duration = Duration::from_millis(50);
/// Stamps sit on a grid of 1/`PAINT_GRID` seconds, and the `PAINT_VARIANTS` steps inside one of
/// its cells name the run's hand. Both are powers of two, so a stamp keeps them exactly up to the
/// glyph clock's wrap: 3,597 seconds of 4,096 steps is under 2^24.
pub const PAINT_GRID: f32 = 128.0;
pub const PAINT_VARIANTS: u32 = 32;
/// The pen's hands, the first variants; a run of new text is written in one of them.
pub const PAINT_HANDS: u32 = 16;
/// The variant a run that replaced text in place soaks in with (a counter ticking, a status
/// changing), and the one the text it replaced fades out with under it: one smooth change rather
/// than a pen, over `PAINT_INLINE` seconds.
pub const PAINT_SOAK: u32 = 16;
pub const PAINT_FADE: u32 = 17;
pub const PAINT_INLINE: f32 = 0.4;
/// The most of a row's characters a change in place may touch at one instant, unless nearly all
/// of the run overwrote text (`PAINT_OVERWRITE_SHARE` of it; a status line whose tail shifted when
/// its timer grew a digit): more is new writing, written by the pen with nothing faded out.
pub const PAINT_INLINE_SHARE: f32 = 0.4;
pub const PAINT_OVERWRITE_SHARE: f32 = 0.6;
/// The variant a working mark is doodled in (Claude Code's spinner, or a tool's blinking dot):
/// one steady mark a pen keeps going round, instead of the frames the program flips through. The
/// cell itself is drawn in `PAINT_HIDE`, which shows nothing, and the terminal paints the steady
/// mark under it. A mark is doodled until `DOODLE_HOLD` seconds after it last changed, easing back
/// to plain ink over the last part, so a dot blinking off is held and a spinner that stops settles.
pub const PAINT_DOODLE: u32 = 18;
pub const PAINT_HIDE: u32 = 19;
/// The variant a cell background is washed in with: a brush sweeping across the band, left to
/// right, over `WASH_TIME` seconds from when its row's text starts. It and the three variants after
/// it say which of the band's top and bottom meet a band of its colour in the next row (`WASH_UP`,
/// `WASH_DOWN`): those edges stay straight so the rows join, and the others are softened.
pub const PAINT_WASH: u32 = 20;
pub const WASH_UP: u32 = 1;
pub const WASH_DOWN: u32 = 2;
pub const WASH_TIME: f32 = 0.3;
pub const DOODLE_HOLD: f32 = 1.0;
/// How many times a second the pen goes round a doodled mark.
pub const DOODLE_TURNS: f32 = 0.9;

/// Claude Code's spinner frames, which a doodle draws as one steady star.
const SPINNER: &[char] = &['·', '✢', '✳', '✶', '✻', '✽'];
/// The dots a running tool blinks, which a doodle draws as one steady dot.
const DOTS: &[char] = &['⏺', '●'];

/// The steady mark a change from `was` to `now` is doodled as, if it is a working mark's: one
/// spinner frame to another (a star), or a dot blinking on or off (a dot). Anything else, a middle
/// dot written as text among them, is no working mark.
pub fn doodle_mark(was: char, now: char) -> Option<char> {
    let dot = |c: char| DOTS.contains(&c);
    if was != now && SPINNER.contains(&was) && SPINNER.contains(&now) {
        Some('✻')
    } else if (dot(was) || dot(now)) && (was == ' ' || dot(was)) && (now == ' ' || dot(now)) {
        Some('⏺')
    } else {
        None
    }
}

/// Whether a cell showing `c` can carry a doodle: a working mark, or a space (a dot blinked off).
pub fn holds_doodle(c: char) -> bool {
    c == ' ' || SPINNER.contains(&c) || DOTS.contains(&c)
}
/// The steps a second a stamp is counted in.
const STEPS: f32 = PAINT_GRID * PAINT_VARIANTS as f32;
/// Runs written at one instant start one after another down the screen: each a little after the
/// one above it, up to a most, and then up to a little more at random.
pub const PAINT_CASCADE: f32 = 0.04;
pub const PAINT_CASCADE_MAX: f32 = 0.2;
pub const PAINT_JITTER: f32 = 0.1;

/// The hand a run is written in, so the lines of one output differ: `[pace, glyph time, across,
/// bead]`. Pace scales the stagger (0.7 to 1.3), glyph time scales `PAINT_DUR` (0.8 to 1.25),
/// across is how much of the pen's path runs across a glyph rather than down it (0.5 to 0.75),
/// and bead how strong its wet bead is (0.6 to 1.2). The soak and the fade, and the spare
/// variants past them, have no stagger and last `PAINT_INLINE`. The shader reads the same table,
/// for every column but the pace.
pub fn hand(variant: u32) -> [f32; 4] {
    if (PAINT_WASH..PAINT_WASH + 4).contains(&variant) {
        return [0.0, WASH_TIME / PAINT_DUR, 0.92, 0.6];
    }
    if variant >= PAINT_HANDS {
        return [0.0, PAINT_INLINE / PAINT_DUR, 0.62, 1.0];
    }
    let h = |k: u32| hash01(variant * 4 + k);
    [0.7 + 0.6 * h(1), 0.8 + 0.45 * h(2), 0.5 + 0.25 * h(3), 0.6 + 0.6 * h(4)]
}

/// The longest glyph time any hand has, which the dry instant allows for.
const GLYPH_TIME_MAX: f32 = 1.25;

/// When (0 to 1 of its time) the pen reaches `u` (0 to 1) of its way along a line, on Material 3's
/// standard decelerate curve `(0, 0, 0, 1)`. That curve is x = s^3, y = 3s^2 - 2s^3 for s from 0
/// to 1, so y is the smoothstep of s, whose inverse has a closed form.
pub fn unease(u: f32) -> f32 {
    let s = 0.5 - ((1.0 - 2.0 * u.clamp(0.0, 1.0)).asin() / 3.0).sin();
    (s * s * s).clamp(0.0, 1.0)
}

/// The lines that tell a glyph shader the paint's timing: `ARKOS_PAINT_DUR`, `ARKOS_PAINT_DRY`,
/// the steps a second a stamp is counted in, the soak's,
/// the fade's, the doodle's and the hidden variants, the doodle's timing and the table of hands,
/// `ARKOS_PAINT_HAND`.
pub fn paint_lines() -> String {
    let hands: Vec<String> = (0..PAINT_VARIANTS)
        .map(|v| {
            let [a, b, c, d] = hand(v);
            format!("float4({a:.6}, {b:.6}, {c:.6}, {d:.6})")
        })
        .collect();
    format!(
        "constant float ARKOS_PAINT_DUR = {PAINT_DUR};\nconstant float ARKOS_PAINT_DRY = {PAINT_DRY};\nconstant float ARKOS_PAINT_STEPS = {STEPS:.1};\nconstant int ARKOS_PAINT_VARIANTS = {PAINT_VARIANTS};\nconstant int ARKOS_PAINT_SOAK = {PAINT_SOAK};\nconstant int ARKOS_PAINT_FADE = {PAINT_FADE};\nconstant int ARKOS_PAINT_DOODLE = {PAINT_DOODLE};\nconstant int ARKOS_PAINT_HIDE = {PAINT_HIDE};\nconstant int ARKOS_PAINT_WASH = {PAINT_WASH};\nconstant float ARKOS_DOODLE_HOLD = {DOODLE_HOLD:.2};\nconstant float ARKOS_DOODLE_TURNS = {DOODLE_TURNS:.2};\nconstant float4 ARKOS_PAINT_HAND[{PAINT_VARIANTS}] = {{{}}};",
        hands.join(", ")
    )
}

/// A stamp for a pen start of `t` on the glyph clock in the hand `variant`: `t` to the nearest
/// grid cell, the variant in the steps inside it (within 12 ms of `t` all told). Never 0, which marks a glyph with no stamp: a
/// start that would be is moved one grid cell on, in the same hand.
pub fn encode(t: f32, variant: u32) -> f32 {
    let stamp = ((t * PAINT_GRID).round() * PAINT_VARIANTS as f32 + variant as f32) / STEPS;
    if stamp == 0.0 { 1.0 / PAINT_GRID } else { stamp }
}

/// The hand a stamp names, as the shader reads it.
pub fn variant_of(stamp: f32) -> u32 {
    ((stamp * STEPS).round() as i32 & (PAINT_VARIANTS as i32 - 1)) as u32
}

/// The stamp a cell background is washed in with, `stamp` being that of the first stamped cell
/// over it and the glyph clock at `clock`: the wash starts with the text over its left end. A band
/// under no stamp (or a doodled or hidden one) still names the wash, so the shader draws it as
/// one, but long dried. `joined` is `WASH_UP` and `WASH_DOWN` as they hold.
pub fn wash_stamp(stamp: f32, clock: f32, joined: u32) -> f32 {
    let variant = variant_of(stamp);
    let wash = PAINT_WASH + (joined & (WASH_UP | WASH_DOWN));
    if stamp == 0.0 || variant == PAINT_DOODLE || variant == PAINT_HIDE {
        return encode(clock - WASH_DRIED, wash);
    }
    encode(stamp, wash)
}

/// How long ago a band that is not washing in was washed, as its stamp says: past any wetness.
const WASH_DRIED: f32 = 30.0;

/// A run's key, from its characters, so a line keeps its hand while it scrolls.
fn run_key(chars: impl Iterator<Item = char>) -> u32 {
    chars.fold(2_166_136_261u32, |h, c| (h ^ c as u32).wrapping_mul(16_777_619))
}

/// One row's stamps, the stamps of the characters its changed cells replaced (0 where there is
/// none to fade out), and when its ink is dry. `changed` is each cell's change instant (None for
/// a cell that never changed or settled at once), `chars` its characters, `was` the characters
/// they replaced, `x` a column's left edge
/// in points from the window's left, `clock` the glyph clock at `now`, and `order` how many runs
/// of each instant the rows above have had. Cells changed at one instant are a run, written in a
/// hand picked by their characters: the first starts at the instant on the glyph clock plus the
/// run's place in the cascade, and each after it a little later, at the hand's pace along the
/// line but on Material's standard decelerate curve (`unease`), and within `PAINT_LINE` however
/// long the run is. Each cell's stamp
/// is its own start. A run that replaced text in place (any of its cells was not a space, and the cells
/// changed at its instant are at most `PAINT_INLINE_SHARE` of the row's characters or at least
/// `PAINT_OVERWRITE_SHARE` of the run's cells were not spaces) is no pen:
/// it soaks in at its instant, all at once, and the text it replaced fades out under it. A cell whose
/// ink is already dry gets 0, which the shader draws as ever, as does an unchanged one. The dry
/// instant is that of the latest-drying cell still wet, None when none is.
pub fn row_stamps(
    changed: &[Option<Instant>],
    chars: &[char],
    was: &[char],
    x: impl Fn(usize) -> f32,
    clock: f32,
    now: Instant,
    order: &mut HashMap<Instant, usize>,
) -> (Vec<f32>, Vec<f32>, Option<Instant>) {
    let mut stamps = vec![0.0; changed.len()];
    let mut ghosts = vec![0.0; changed.len()];
    let mut dry: Option<Instant> = None;
    let mut done = vec![false; changed.len()];
    for first in 0..changed.len() {
        let Some(at) = changed[first] else { continue };
        if done[first] {
            continue;
        }
        let same: Vec<usize> = (first..changed.len()).filter(|&c| changed[c] == Some(at)).collect();
        let last = *same.last().unwrap_or(&first);
        let since = now.saturating_duration_since(at).as_secs_f32();
        let was_at = |c: usize| was.get(c).copied().unwrap_or(' ');
        let touched = changed.iter().filter(|&&c| c == Some(at)).count() as f32;
        let written = chars.iter().filter(|&&c| c != ' ').count().max(1) as f32;
        let over = same.iter().filter(|&&c| was_at(c) != ' ').count() as f32;
        let inline = over > 0.0 && (touched <= written * PAINT_INLINE_SHARE || over >= same.len() as f32 * PAINT_OVERWRITE_SHARE);
        // Each of the run's cells' stamps, in order.
        let (starts, ghost, wet) = if inline {
            let t = clock - since;
            (vec![encode(t, PAINT_SOAK); same.len()], encode(t, PAINT_FADE), PAINT_INLINE + PAINT_DRY)
        } else {
            let key = run_key(same.iter().map(|&c| chars.get(c).copied().unwrap_or(' ')));
            let variant = (hash01(key) * PAINT_HANDS as f32) as u32 % PAINT_HANDS;
            let [pace, ..] = hand(variant);
            let nth = order.entry(at).or_insert(0);
            let delay = if *nth == 0 { 0.0 } else { (*nth as f32 * PAINT_CASCADE).min(PAINT_CASCADE_MAX) + hash01(key ^ 0x5bd1_e995) * PAINT_JITTER };
            *nth += 1;
            // How long the starts would spread at the hand's even pace, and how long they do.
            let even = (x(last) - x(first)) * PAINT_STAGGER_PER_PT * pace;
            let spread = even.min(PAINT_LINE - PAINT_DUR);
            let (x0, xn, t) = (x(first), x(last), clock - since + delay);
            let wet = delay + spread + PAINT_DUR * GLYPH_TIME_MAX + PAINT_DRY;
            let starts = same
                .iter()
                .map(|&c| {
                    let along = if xn > x0 { (x(c) - x0) / (xn - x0) } else { 0.0 };
                    encode(t + spread * unease(along), variant)
                })
                .collect();
            (starts, 0.0, wet)
        };
        let ends = at + Duration::from_secs_f32(wet);
        for (&c, &start) in same.iter().zip(&starts) {
            done[c] = true;
            if ends > now {
                stamps[c] = start;
                if was_at(c) != ' ' {
                    ghosts[c] = ghost;
                }
            }
        }
        if ends > now {
            dry = dry.max(Some(ends));
        }
    }
    (stamps, ghosts, dry)
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

    fn stamps(changed: &[Option<Instant>], clock: f32, now: Instant) -> (Vec<f32>, Option<Instant>) {
        let chars: Vec<char> = "Step 1: update the parser".chars().collect();
        let (s, ghosts, dry) = row_stamps(changed, &chars, &[], x, clock, now, &mut HashMap::new());
        assert!(ghosts.iter().all(|&g| g == 0.0), "new text replaced nothing");
        (s, dry)
    }

    #[test]
    fn cells_changed_together_start_one_after_another_from_the_first_at_its_instant() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(100);
        let changed = [None, Some(t0), Some(t0), Some(now), None];
        let (s, dry) = stamps(&changed, 50.0, now);
        assert_eq!((s[0], s[4]), (0.0, 0.0));
        assert_eq!(variant_of(s[1]), variant_of(s[2]), "one run, one hand");
        // A stamp is the glyph's own start.
        let grid = 1.5 / PAINT_GRID;
        assert!((s[1] - 49.9).abs() <= grid, "{}", s[1]);
        let gap = s[2] - s[1];
        assert!((0.7 * 0.011 - 2.0 * grid..=1.3 * 0.011 + 2.0 * grid).contains(&gap), "{gap}");
        assert!((s[3] - 50.0).abs() <= grid, "a cell changed now starts now, wherever it is");
        assert!(dry.is_some_and(|d| d > now + Duration::from_secs_f32(PAINT_DRY)));
    }

    /// The starts of a line `len` cells long written at one instant, from its first.
    fn starts(len: usize) -> Vec<f32> {
        let t0 = Instant::now();
        let chars: Vec<char> = (0..len).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let (s, _, _) = row_stamps(&vec![Some(t0); len], &chars, &[], x, 10.0, t0, &mut HashMap::new());
        s.iter().map(|&v| v - s[0]).collect()
    }

    #[test]
    fn a_long_line_is_written_within_the_line_time_however_long() {
        let grid = 1.5 / PAINT_GRID;
        for len in [60, 120, 400] {
            let s = starts(len);
            assert!(s.windows(2).all(|w| w[1] >= w[0]), "left to right: {s:?}");
            let last = s[len - 1];
            assert!(last <= PAINT_LINE - PAINT_DUR + grid, "{len}: {last}");
            assert!(last >= PAINT_LINE - PAINT_DUR - grid, "a long line takes all of it: {len}: {last}");
        }
    }

    #[test]
    fn a_short_line_keeps_the_pen_s_own_pace() {
        let s = starts(8);
        let last = s[7];
        let linear = 7.0 * 9.6 * PAINT_STAGGER_PER_PT;
        assert!((0.7 * linear - 2.0 / PAINT_GRID..=1.3 * linear + 2.0 / PAINT_GRID).contains(&last), "{last}");
    }

    #[test]
    fn a_line_starts_quickly_and_settles_toward_its_end() {
        let s = starts(200);
        assert!(s[100] < 0.45 * s[199], "half the letters start well inside the first half: {} of {}", s[100], s[199]);
    }

    #[test]
    fn the_pen_paces_a_line_on_material_s_standard_decelerate_curve() {
        use crate::motion::{cubic_bezier, STANDARD_DECELERATE};
        for u in [0.0, 0.1, 0.37, 0.5, 0.9, 1.0] {
            assert!((cubic_bezier(STANDARD_DECELERATE, unease(u)) - u).abs() < 1e-4, "{u}");
        }
    }

    #[test]
    fn a_stamp_carries_its_hand_exactly_up_to_the_clock_s_wrap() {
        for t in [0.0, 0.3, 17.25, 1800.7, 3596.9] {
            for v in 0..PAINT_VARIANTS {
                let s = encode(t, v);
                assert_eq!(variant_of(s), v, "{t} {v}");
                assert!((s - t).abs() <= 1.5 / PAINT_GRID, "{t} {s}");
            }
        }
        assert_ne!(encode(0.0, 0), 0.0, "a stamp never lands on the dry mark");
    }

    #[test]
    fn lines_written_at_once_cascade_down_the_screen_in_hands_of_their_own() {
        let t0 = Instant::now();
        let mut order = HashMap::new();
        let lines = ["Step 1: I will update the parser.", "  Then the tests for step 1.", "  And a note in CHANGELOG.md."];
        let firsts: Vec<f32> = lines
            .iter()
            .map(|l| {
                let chars: Vec<char> = l.chars().collect();
                let changed = vec![Some(t0); chars.len()];
                let (s, _, _) = row_stamps(&changed, &chars, &[], |_| 0.0, 10.0, t0, &mut order);
                s[0]
            })
            .collect();
        assert!((firsts[0] - 10.0).abs() <= 1.5 / PAINT_GRID, "the top line starts at once");
        assert!(firsts[1] > firsts[0] && firsts[2] > firsts[0], "{firsts:?}");
        let hands: std::collections::HashSet<u32> = firsts.iter().map(|&s| variant_of(s)).collect();
        assert!(hands.len() > 1, "the lines are not all in one hand");
        assert_eq!(order[&t0], 3);
    }

    #[test]
    fn every_hand_stays_in_its_ranges_and_the_shader_is_told_them_all() {
        for v in 0..PAINT_HANDS {
            let [pace, time, across, bead] = hand(v);
            assert!((0.7..=1.3).contains(&pace) && (0.8..=GLYPH_TIME_MAX).contains(&time));
            assert!((0.5..=0.75).contains(&across) && (0.6..=1.2).contains(&bead));
        }
        let lines = paint_lines();
        assert!(lines.contains("constant float ARKOS_PAINT_STEPS = 4096.0;"), "{lines}");
        assert!(!lines.contains("STAGGER"), "the shader adds no stagger: {lines}");
        assert_eq!(lines.matches("float4(").count(), PAINT_VARIANTS as usize);
    }

    #[test]
    fn text_replaced_in_place_soaks_in_at_once_over_the_text_it_replaced_fading_out() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(100);
        // "Step 4:" became "Step 5:": only the digit changed, and it replaced a 4.
        let chars: Vec<char> = "Step 5:".chars().collect();
        let was: Vec<char> = "     4 ".chars().collect();
        let mut changed = vec![None; 7];
        changed[5] = Some(t0);
        let (s, ghosts, dry) = row_stamps(&changed, &chars, &was, x, 20.0, now, &mut HashMap::new());
        assert_eq!(variant_of(s[5]), PAINT_SOAK);
        assert_eq!(variant_of(ghosts[5]), PAINT_FADE);
        assert!((s[5] - 19.9).abs() <= 1.5 / PAINT_GRID, "it starts at its instant, wherever it is: {}", s[5]);
        assert_eq!(hand(PAINT_SOAK)[0], 0.0, "no stagger");
        assert!(s.iter().enumerate().all(|(c, &v)| c == 5 || v == 0.0) && ghosts.iter().enumerate().all(|(c, &v)| c == 5 || v == 0.0));
        assert_eq!(dry, Some(t0 + Duration::from_secs_f32(PAINT_INLINE + PAINT_DRY)));
        let later = t0 + Duration::from_secs(2);
        assert_eq!(row_stamps(&changed, &chars, &was, x, 22.0, later, &mut HashMap::new()), (vec![0.0; 7], vec![0.0; 7], None), "once dry, neither is drawn");
    }

    #[test]
    fn a_row_mostly_rewritten_after_a_scroll_is_new_writing_and_fades_nothing_out() {
        // A scroll put a new line on a row that held another: most of its cells differ, and the
        // decode says they replaced nothing, since the old line moved on.
        let t0 = Instant::now();
        let chars: Vec<char> = "-r-xr-xr-x 1 root wheel 580 bundle".chars().collect();
        let old: Vec<char> = "-rwxr-xr-x 2 root wheel 7013 json".chars().chain("  ".chars()).collect();
        let changed: Vec<Option<Instant>> = chars.iter().zip(&old).map(|(a, b)| (a != b).then_some(t0)).collect();
        let was = vec![' '; chars.len()];
        let (s, ghosts, _) = row_stamps(&changed, &chars, &was, x, 20.0, t0, &mut HashMap::new());
        assert!(s.iter().filter(|&&v| v != 0.0).all(|&v| variant_of(v) < PAINT_HANDS), "the pen writes it");
        assert!(ghosts.iter().all(|&g| g == 0.0), "nothing fades out");
    }

    #[test]
    fn a_tail_shifted_over_its_old_self_crossfades_rather_than_being_rewritten() {
        // "(9s · 4.0k)" became "(10s · 4.0k)": every cell from the digit on changed, over text.
        let t0 = Instant::now();
        let chars: Vec<char> = "x (10s · 4.0k)".chars().collect();
        let was: Vec<char> = "x (9s · 4.0k) ".chars().collect();
        // As the decode marks them: a cell that became a space is not a change.
        let changed: Vec<Option<Instant>> = chars.iter().zip(&was).map(|(a, b)| (a != b && *a != ' ').then_some(t0)).collect();
        let (s, ghosts, _) = row_stamps(&changed, &chars, &was, x, 20.0, t0, &mut HashMap::new());
        assert!(s.iter().filter(|&&v| v != 0.0).all(|&v| variant_of(v) == PAINT_SOAK), "{s:?}");
        assert!(ghosts.iter().any(|&g| g != 0.0));
    }

    #[test]
    fn only_a_spinner_s_frames_and_a_blinking_dot_are_doodled() {
        assert_eq!(doodle_mark('✢', '✳'), Some('✻'), "the spinner turns");
        assert_eq!(doodle_mark('·', '✢'), Some('✻'));
        assert_eq!(doodle_mark('⏺', ' '), Some('⏺'), "a dot blinks off");
        assert_eq!(doodle_mark(' ', '⏺'), Some('⏺'), "and on");
        assert_eq!(doodle_mark(' ', '·'), None, "a middle dot written as text");
        assert_eq!(doodle_mark('a', '⏺'), None);
        assert_eq!(doodle_mark('✻', 'W'), None);
        assert!(holds_doodle(' ') && holds_doodle('✽') && holds_doodle('●') && !holds_doodle('x'));
    }

    #[test]
    fn a_background_washes_in_from_where_its_text_starts_and_a_still_one_does_not() {
        let pen = encode(10.0, 3);
        let wash = wash_stamp(pen, 12.0, 0);
        assert_eq!(variant_of(wash), PAINT_WASH);
        assert!((wash - 10.0).abs() <= 1.5 / PAINT_GRID, "it starts with the text over its left end: {wash}");
        assert_eq!(hand(PAINT_WASH)[0], 0.0, "the whole band sweeps at once, not cell by cell");
        let dried = wash_stamp(0.0, 50.0, 0);
        assert_eq!(variant_of(dried), PAINT_WASH, "a still band is still a wash");
        assert!(dried < 50.0 - 10.0, "and long dry: {dried}");
        assert_eq!(variant_of(wash_stamp(encode(10.0, PAINT_HIDE), 50.0, 0)), PAINT_WASH);
        assert_eq!(variant_of(wash_stamp(pen, 12.0, WASH_UP | WASH_DOWN)), PAINT_WASH + 3, "the variant carries which edges join");
        assert_eq!(hand(PAINT_WASH + 3), hand(PAINT_WASH));
    }

    #[test]
    fn dry_ink_draws_as_ever_and_the_row_is_dry() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(2);
        let (s, dry) = stamps(&[Some(t0), Some(t0), None], 50.0, now);
        assert_eq!(s, vec![0.0, 0.0, 0.0]);
        assert_eq!(dry, None);
        assert_eq!(stamps(&[], 50.0, now), (vec![], None));
    }

    #[test]
    fn a_long_run_stays_wet_until_its_last_cell_dries() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(1000);
        let (s, dry) = stamps(&vec![Some(t0); 80], 50.0, now);
        assert!(s.iter().all(|&s| s != 0.0), "the run's end is still wet");
        assert!(dry.is_some_and(|d| d > now));
    }

    #[test]
    fn the_terminal_paints_only_under_a_painting_effect_with_the_clock_running() {
        assert_eq!(paint_clock(true, false, false, 3.5), Some(3.5));
        assert_eq!(paint_clock(false, false, false, 3.5), None);
        assert_eq!(paint_clock(true, true, false, 3.5), None);
        assert_eq!(paint_clock(true, false, true, 3.5), None);
    }
}
