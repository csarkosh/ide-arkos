//! The terminal's paint-in, watercolor's counterpart to the decode: a cell the program writes
//! is written in, as by a pen. The CPU only stamps: each row's cells changed at one instant share a
//! stamp, the instant on the glyph clock less the stagger to the row's first changed cell, which
//! also names the hand the run is written in, and the glyph shader paints from it
//! (`watercolor/glyphs.metal`). Pure rules, no drawing.
use super::hash::hash01;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the pen takes to write one cell's glyph, in seconds, for a hand of glyph time 1.
pub const PAINT_DUR: f32 = 0.08;
/// How long freshly written ink takes to dry, in seconds.
pub const PAINT_DRY: f32 = 0.8;
/// How much later each cell to the right starts, in seconds a point, for a hand of pace 1: 22 ms
/// a cell of the default terminal's 9.6 point wide cells.
pub const PAINT_STAGGER_PER_PT: f32 = 0.022 / 9.6;
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
/// variants past them, have no stagger and last `PAINT_INLINE`. The shader reads the same table.
pub fn hand(variant: u32) -> [f32; 4] {
    if variant >= PAINT_HANDS {
        return [0.0, PAINT_INLINE / PAINT_DUR, 0.62, 1.0];
    }
    let h = |k: u32| hash01(variant * 4 + k);
    [0.7 + 0.6 * h(1), 0.8 + 0.45 * h(2), 0.5 + 0.25 * h(3), 0.6 + 0.6 * h(4)]
}

/// The longest glyph time any hand has, which the dry instant allows for.
const GLYPH_TIME_MAX: f32 = 1.25;

/// The lines that tell a glyph shader the paint's timing: `ARKOS_PAINT_DUR`, `ARKOS_PAINT_DRY`,
/// `ARKOS_PAINT_STAGGER` (seconds a point), the steps a second a stamp is counted in, the soak's,
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
        "constant float ARKOS_PAINT_DUR = {PAINT_DUR};\nconstant float ARKOS_PAINT_DRY = {PAINT_DRY};\nconstant float ARKOS_PAINT_STAGGER = {PAINT_STAGGER_PER_PT:.8};\nconstant float ARKOS_PAINT_STEPS = {STEPS:.1};\nconstant int ARKOS_PAINT_VARIANTS = {PAINT_VARIANTS};\nconstant int ARKOS_PAINT_SOAK = {PAINT_SOAK};\nconstant int ARKOS_PAINT_FADE = {PAINT_FADE};\nconstant int ARKOS_PAINT_DOODLE = {PAINT_DOODLE};\nconstant int ARKOS_PAINT_HIDE = {PAINT_HIDE};\nconstant float ARKOS_DOODLE_HOLD = {DOODLE_HOLD:.2};\nconstant float ARKOS_DOODLE_TURNS = {DOODLE_TURNS:.2};\nconstant float4 ARKOS_PAINT_HAND[{PAINT_VARIANTS}] = {{{}}};",
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
#[cfg(test)]
pub fn variant_of(stamp: f32) -> u32 {
    ((stamp * STEPS).round() as i32 & (PAINT_VARIANTS as i32 - 1)) as u32
}

/// A run's key, from its characters, so a line keeps its hand while it scrolls.
fn run_key(chars: impl Iterator<Item = char>) -> u32 {
    chars.fold(2_166_136_261u32, |h, c| (h ^ c as u32).wrapping_mul(16_777_619))
}

/// One row's stamps, the stamps of the characters its changed cells replaced (0 where there is
/// none to fade out), and when its ink is dry. `changed` is each cell's change instant (None for
/// a cell that never changed or settled at once), `chars` its characters, `was` the characters
/// they replaced, `x` a column's left edge
/// in points from the window's left, `clock` the glyph clock at `now`, and `order` how many runs
/// of each instant the rows above have had. Cells changed at one instant share a stamp, in a hand
/// picked by their characters: the instant on the glyph clock, plus the run's place in the
/// cascade, less the hand's stagger up to the first of them, so the shader, which adds each
/// glyph's own x times that stagger, starts the first cell then and each one after it a little
/// later. A run that replaced text in place (any of its cells was not a space, and the cells
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
        let (stamp, ghost, wet) = if inline {
            let t = clock - since;
            (encode(t, PAINT_SOAK), encode(t, PAINT_FADE), PAINT_INLINE + PAINT_DRY)
        } else {
            let key = run_key(same.iter().map(|&c| chars.get(c).copied().unwrap_or(' ')));
            let variant = (hash01(key) * PAINT_HANDS as f32) as u32 % PAINT_HANDS;
            let [pace, ..] = hand(variant);
            let nth = order.entry(at).or_insert(0);
            let delay = if *nth == 0 { 0.0 } else { (*nth as f32 * PAINT_CASCADE).min(PAINT_CASCADE_MAX) + hash01(key ^ 0x5bd1_e995) * PAINT_JITTER };
            *nth += 1;
            let stagger = PAINT_STAGGER_PER_PT * pace;
            let wet = delay + PAINT_DUR * GLYPH_TIME_MAX + PAINT_DRY + (x(last) - x(first)) * stagger;
            (encode(clock - since + delay - x(first) * stagger, variant), 0.0, wet)
        };
        let ends = at + Duration::from_secs_f32(wet);
        for c in same {
            done[c] = true;
            if ends > now {
                stamps[c] = stamp;
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
    fn cells_changed_together_share_a_stamp_that_starts_the_first_at_its_instant() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(100);
        let changed = [None, Some(t0), Some(t0), Some(now), None];
        let (s, dry) = stamps(&changed, 50.0, now);
        assert_eq!((s[0], s[4]), (0.0, 0.0));
        assert_eq!(s[1], s[2]);
        // The shader starts a glyph at its stamp plus its x times its hand's stagger.
        let start = |c: usize| s[c] + x(c) * PAINT_STAGGER_PER_PT * hand(variant_of(s[c]))[0];
        let grid = 1.5 / PAINT_GRID;
        assert!((start(1) - 49.9).abs() <= grid, "{}", start(1));
        let gap = start(2) - start(1);
        assert!((0.7 * 0.022 - 1e-4..=1.3 * 0.022 + 1e-4).contains(&gap), "{gap}");
        assert!((start(3) - 50.0).abs() <= grid, "a cell changed now starts now, wherever it is");
        assert!(dry.is_some_and(|d| d > now + Duration::from_secs_f32(PAINT_DRY)));
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
