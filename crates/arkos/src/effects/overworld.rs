//! The built-in `overworld` effect's layout and timing, mirrored from its wash shader
//! (`effects/overworld/wash.metal`) so they can be tested without a GPU. The constants and the
//! arithmetic here are the shader's own; a change to one is made to both.

pub use super::hash::hash01;
use crate::tree::Rect;

/// An art pixel's side, in points. The wash draws at `scale = 1.0`, and every art pixel is
/// snapped to whole points from its band's origin, so its edges land on window pixels.
pub const ART_PX: f32 = 3.0;
/// The tallest the scene is drawn at its own size, in points: its 46 art rows and 10 more of open
/// sky fit under it. Drawn at twice its size (`scene_px`) it may be twice as tall.
pub const SCENE_MAX_H: f32 = 170.0;
/// A sidebar with at least this much room inside its frame, and room across for
/// `SCENE_TALL_COLS` art pixels of twice the size, has its scene drawn at twice its size, so the
/// art reaches further up a tall sidebar while its pixels stay whole.
pub const SCENE_TALL_ROOM: f32 = 640.0;
pub const SCENE_TALL_COLS: f32 = 34.0;
/// Under the sidebar's names the scene is softened for their contrast, and below the last of
/// them it comes back to full colour over this many points, a row of art at a time.
pub const SOFT_BUFFER: f32 = 24.0;
/// A sidebar with less room than this inside its frame has no scene at all.
pub const SCENE_MIN_H: f32 = 50.0;
/// A frame's width in art pixels: a pane's in the gap round it (so the manifest's `gap` is two
/// frames and one art pixel of ground between them), the sidebar's and the title bar's inside
/// their own edges.
pub const FRAME_PX: i32 = 2;
/// How far, in points, a title bar star keeps from each of the bar's word and icon groups.
pub const STAR_CLEAR: f32 = 12.0;

/// Seconds from one castle window's dimming to the next, give or take half of `DIM_SLACK`, so
/// two are 3 to 7 seconds apart. Each dims for `DIM_FOR` and comes back.
pub const DIM_CYCLE: f32 = 5.0;
pub const DIM_SLACK: f32 = 2.0;
pub const DIM_FOR: f32 = 0.8;
/// The castle's lit windows, which the shader draws in this order.
pub const CASTLE_WINDOWS: usize = 5;
/// A salt that gives the castle windows' dimming hashes of its own.
pub const DIM_SALT: u32 = 0x85EB_CA6B;

/// How many fireflies rise through the scene.
pub const FIREFLIES: usize = 6;
/// The slowest and quickest a firefly takes to rise through the scene, in seconds: each has
/// its own, so they drift apart.
pub const FIREFLY_PERIOD: [f32; 2] = [8.0, 11.0];
/// A firefly fades in over the first `FIREFLY_FADE_IN` of its rise, is fully lit until
/// `FIREFLY_FADE_FROM`, fades out by `FIREFLY_FADE_TO` and stays dark for the rest, so it is gone
/// before it leaves the scene.
pub const FIREFLY_FADE_IN: f32 = 0.1;
pub const FIREFLY_FADE_FROM: f32 = 0.8;
pub const FIREFLY_FADE_TO: f32 = 0.95;
/// A salt that gives the fireflies hashes of their own.
pub const FIREFLY_SALT: u32 = 0x27D4_EB2F;

/// The shortest and longest a star's twinkle takes, in seconds.
pub const TWINKLE_PERIOD: [f32; 2] = [2.0, 6.0];
/// A salt that gives the stars' twinkles hashes of their own.
pub const TWINKLE_SALT: u32 = 0x1656_67B1;

/// The band of the sidebar `side` the scene is drawn in: inside its frame, at most `SCENE_MAX_H`
/// tall in whole art pixels, standing on the frame's foot whatever the sidebar's names cover (it
/// is softened under them). `None` in a sidebar with under `SCENE_MIN_H` of room inside its frame.
pub fn scene_band(side: Rect) -> Option<Rect> {
    let frame = FRAME_PX as f32 * ART_PX;
    let room = side.h - 2.0 * frame;
    if room < SCENE_MIN_H || side.w <= 2.0 * frame {
        return None;
    }
    let px = scene_px(side);
    let h = (room.min(SCENE_MAX_H * px / ART_PX) / px).floor() * px;
    Some(Rect { x: side.x + frame, y: side.y + side.h - frame - h, w: side.w - 2.0 * frame, h })
}

/// The side of one of the scene's art pixels in the sidebar `side`, in points: `ART_PX`, or twice
/// that in a sidebar tall and wide enough (`SCENE_TALL_ROOM`, `SCENE_TALL_COLS`).
pub fn scene_px(side: Rect) -> f32 {
    let frame = FRAME_PX as f32 * ART_PX;
    let tall = side.h - 2.0 * frame >= SCENE_TALL_ROOM && side.w - 2.0 * frame >= SCENE_TALL_COLS * 2.0 * ART_PX;
    if tall {
        2.0 * ART_PX
    } else {
        ART_PX
    }
}

/// How far the scene's art row whose top is at `row_top` is softened toward the sidebar's colour,
/// with the names ending at `names_end`: all the way for a row the names reach into, then less a
/// row at a time over `SOFT_BUFFER` below them, and not at all past that.
pub fn soft_amount(row_top: f32, names_end: f32) -> f32 {
    if row_top < names_end {
        return 1.0;
    }
    (1.0 - (row_top - names_end) / SOFT_BUFFER).clamp(0.0, 1.0)
}

/// Whether a title bar star may be drawn at `p`: outside every group grown by `STAR_CLEAR` on
/// every side.
pub fn star_clear(p: (f32, f32), groups: &[Rect]) -> bool {
    !groups.iter().any(|g| {
        p.0 >= g.x - STAR_CLEAR && p.0 <= g.x + g.w + STAR_CLEAR && p.1 >= g.y - STAR_CLEAR && p.1 <= g.y + g.h + STAR_CLEAR
    })
}

/// The title bar's stars: none before `TITLE_LEFT`, the traffic lights' end; one in most of the
/// cells of `TITLE_CELL` art pixels, on a row from 2 to 6 of the bar, inside its frame, the whole
/// length of the bar. One within `STAR_CLEAR` of the bar's words or icons is softened into the
/// bar's colour, as the scene is under the sidebar's names.
pub const TITLE_LEFT: f32 = 80.0;
/// The bar's words, its icons and the traffic lights each sit on a plate of the bar's plain
/// colour, `PLATE_PAD` points wider than what it holds on each side.
pub const PLATE_PAD: f32 = 9.0;
pub const TITLE_CELL: i32 = 8;
pub const TITLE_STAR_CHANCE: f32 = 0.6;
/// A salt that gives the title bar's stars hashes of their own.
pub const TITLE_SALT: u32 = 0x9E37_79B9;

/// The art pixels, counted from the bar's origin, of the stars drawn in a title bar laid out at
/// `bar`, as the shader draws them: in each cell that wins its chance, at a seeded place, right of
/// the traffic lights and whole inside the bar. With each, whether it keeps clear of every one of
/// the bar's `groups`: one that does not is softened.
pub fn title_stars(bar: Rect, groups: &[Rect]) -> Vec<((i32, i32), bool)> {
    let cells = (bar.w / ART_PX).floor() as i32 / TITLE_CELL + 1;
    (0..cells)
        .filter_map(|cell| {
            let id = cell as u32 ^ TITLE_SALT;
            let star = (cell * TITLE_CELL + 2 + (hash01(id.wrapping_add(1)) * (TITLE_CELL - 4) as f32) as i32, 2 + (hash01(id.wrapping_add(2)) * 5.0) as i32);
            if hash01(id) >= TITLE_STAR_CHANCE {
                return None;
            }
            let o = (bar.x + star.0 as f32 * ART_PX, bar.y + star.1 as f32 * ART_PX);
            let corners = [o, (o.0 + ART_PX, o.1), (o.0, o.1 + ART_PX), (o.0 + ART_PX, o.1 + ART_PX)];
            let clear = !groups.is_empty() && corners.iter().all(|c| star_clear(*c, groups));
            (o.0 >= TITLE_LEFT && o.0 + ART_PX <= bar.x + bar.w).then_some((star, clear))
        })
        .collect()
}

/// The cloud in the title bar: `CLOUD_W` art pixels wide and 3 tall from row `CLOUD_ROW`,
/// crossing the bar right of `TITLE_LEFT` at `CLOUD_SPEED` art pixels a second (a 1200 point bar
/// in about a minute and a half: ambient, a period of its own) from `CLOUD_START` of the way along.
pub const CLOUD_W: i32 = 12;
pub const CLOUD_ROW: i32 = 2;
pub const CLOUD_SPEED: f32 = 4.0;
/// A second, smaller cloud: `CLOUD2_W` wide and two rows tall from the row under `CLOUD_ROW`, on
/// the first one's course `CLOUD2_RATE` as fast and `CLOUD2_AHEAD` seconds along it.
pub const CLOUD2_W: i32 = 8;
pub const CLOUD2_RATE: f32 = 0.6;
pub const CLOUD2_AHEAD: f32 = 150.0;
pub const CLOUD_START: f32 = 0.6;

/// The cloud's left edge at `t` seconds, in art pixels from the bar's left, for a bar `w` art
/// pixels wide: it enters whole from behind the traffic lights' end and leaves past the right.
pub fn cloud_at(w: i32, t: f32) -> i32 {
    let from = (TITLE_LEFT / ART_PX).ceil() as i32;
    let span = (w - from + CLOUD_W).max(1) as f32;
    from - CLOUD_W + ((CLOUD_START * span + t * CLOUD_SPEED) % span) as i32
}

/// A working pane's glint: `GLINT_LEN` art pixels of the frame's outer line, going round at
/// `GLINT_SPEED` art pixels a second (ambient, a period of its own).
pub const GLINT_LEN: f32 = 6.0;
pub const GLINT_SPEED: f32 = 12.0;
/// A waiting pane's "continue" arrow: `ARROW_W` art pixels wide at its top and three rows tall,
/// `ARROW_IN` in from its body's right and foot, bobbing one art pixel down and back every
/// `ARROW_BOB` seconds (extra-long2, 800 ms, each way; ambient).
pub const ARROW_W: i32 = 5;
pub const ARROW_IN: i32 = 4;
pub const ARROW_BOB: f32 = 0.8;
/// The cursor on the selected row bobs one art pixel toward the row and back every `CURSOR_BOB`
/// seconds (long2, 500 ms, each way; ambient).
pub const CURSOR_BOB: f32 = 0.5;
/// The manifest's `paper_alpha`, which the wash draws its night through.
pub const PAPER: f32 = 0.6;
/// How far toward the dim text a star behind a terminal reaches the screen at its brightest.
pub const STAR_LIMIT: f32 = 0.25;

/// Which ring of the frame round the pane `r` the point `p` is in, each an art pixel wide: 0
/// inside the pane, 1 the inner line, 2 the outer line, more beyond the frame; and whether `p`
/// is on a corner of its ring.
pub fn frame_ring(p: (f32, f32), r: Rect) -> (i32, bool) {
    let out = |lo: f32, hi: f32, v: f32| ((lo - v).max(v - hi).max(0.0) / ART_PX).ceil() as i32;
    let (ax, ay) = (out(r.x, r.x + r.w, p.0), out(r.y, r.y + r.h, p.1));
    let ring = ax.max(ay);
    (ring, ax == ring && ay == ring)
}

/// Where dimming `k` (from 1) starts, in seconds of the effect's clock.
fn dim_start(k: u32) -> f32 {
    k as f32 * DIM_CYCLE + (hash01(k ^ DIM_SALT) - 0.5) * DIM_SLACK
}

/// The castle window dimmed at `t` seconds, if one is. Cycle 0 has no dimming, so the clock's
/// zero, the still frame under Reduce Motion, has every window lit.
pub fn dimmed_window(t: f32) -> Option<usize> {
    // Only the dimming of the cycle nearest `t` can be under way: each starts within half the
    // slack of its cycle's mark and is over long before the next could start.
    let k = ((t + 0.5 * DIM_SLACK) / DIM_CYCLE).floor();
    if k < 1.0 {
        return None;
    }
    let k = k as u32;
    let start = dim_start(k);
    (t >= start && t < start + DIM_FOR).then(|| ((hash01((k ^ DIM_SALT).wrapping_add(1)) * CASTLE_WINDOWS as f32) as usize).min(CASTLE_WINDOWS - 1))
}

/// Every dimming's start up to `t` seconds, in order.
pub fn dim_starts_until(t: f32) -> Vec<f32> {
    (1..).map(dim_start).take_while(|s| *s <= t).collect()
}

/// Firefly `i` at `t` seconds: how far through its rise it is (0 at the grass, 1 at the top of
/// its climb) and how lit. Its phase is set so the clock's zero finds it fully lit, and the
/// three are spread through their rises.
pub fn firefly(i: usize, t: f32) -> (f32, f32) {
    let id = i as u32 ^ FIREFLY_SALT;
    let period = FIREFLY_PERIOD[0] + (FIREFLY_PERIOD[1] - FIREFLY_PERIOD[0]) * hash01(id);
    let phase = FIREFLY_FADE_IN + (FIREFLY_FADE_FROM - FIREFLY_FADE_IN) * (i as f32 + 0.5) / FIREFLIES as f32;
    let x = t / period + phase;
    let rise = x - x.floor();
    (rise, firefly_alpha(rise))
}

/// How lit a firefly is `rise` of the way through its climb.
fn firefly_alpha(rise: f32) -> f32 {
    if rise < FIREFLY_FADE_IN {
        rise / FIREFLY_FADE_IN
    } else if rise < FIREFLY_FADE_FROM {
        1.0
    } else if rise < FIREFLY_FADE_TO {
        1.0 - (rise - FIREFLY_FADE_FROM) / (FIREFLY_FADE_TO - FIREFLY_FADE_FROM)
    } else {
        0.0
    }
}

/// A star's brightness at `t` seconds, 0.2 to 1: a slow cosine with a period of its own, so
/// every star is at full brightness at the clock's zero.
pub fn twinkle(seed: f32, t: f32) -> f32 {
    let period = TWINKLE_PERIOD[0] + (TWINKLE_PERIOD[1] - TWINKLE_PERIOD[0]) * hash01(seed as u32 ^ TWINKLE_SALT);
    0.6 + 0.4 * (std::f32::consts::TAU * t / period).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_stands_on_the_sidebar_s_frame_up_to_its_full_height() {
        let band = scene_band(Rect { x: 0.0, y: 28.0, w: 240.0, h: 600.0 }).unwrap();
        assert_eq!(band, Rect { x: 6.0, y: 628.0 - 6.0 - 168.0, w: 228.0, h: 168.0 }, "56 art pixels, the most under 170 points, inside the frame");
    }

    #[test]
    fn a_tall_wide_sidebar_draws_the_scene_at_twice_its_size_in_whole_art_pixels() {
        let tall = Rect { x: 0.0, y: 28.0, w: 260.0, h: 900.0 };
        assert_eq!(scene_px(tall), 2.0 * ART_PX);
        let band = scene_band(tall).unwrap();
        assert_eq!(band.h, 336.0, "56 rows of six points: twice the scene's height");
        assert_eq!(band.y + band.h, tall.y + tall.h - FRAME_PX as f32 * ART_PX, "on the frame's foot");
        assert_eq!(band.h % (2.0 * ART_PX), 0.0);
        // Short or narrow, the scene keeps its own size.
        assert_eq!(scene_px(Rect { h: 600.0, ..tall }), ART_PX);
        assert_eq!(scene_px(Rect { w: 200.0, ..tall }), ART_PX, "under 34 columns of six points across");
        assert_eq!(scene_band(Rect { h: 600.0, ..tall }).unwrap().h, 168.0);
    }

    #[test]
    fn the_scene_is_softened_under_the_names_and_comes_back_over_a_buffer_below_them() {
        assert_eq!(soft_amount(100.0, 120.0), 1.0, "a row the names reach into");
        assert_eq!(soft_amount(119.0, 120.0), 1.0);
        assert_eq!(soft_amount(120.0, 120.0), 1.0, "the first row below starts from all the way");
        assert_eq!(soft_amount(132.0, 120.0), 0.5);
        assert_eq!(soft_amount(144.0, 120.0), 0.0, "past the buffer, full colour");
        assert_eq!(soft_amount(400.0, 120.0), 0.0);
        let steps: Vec<f32> = (0..10).map(|n| soft_amount(120.0 + 3.0 * n as f32, 120.0)).collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0]), "it only lightens going down: {steps:?}");
    }

    #[test]
    fn a_short_sidebar_cuts_the_scene_from_the_top_in_whole_art_pixels_and_a_tiny_one_has_none() {
        let band = scene_band(Rect { x: 0.0, y: 28.0, w: 240.0, h: 103.0 }).unwrap();
        assert_eq!((band.y, band.h), (28.0 + 103.0 - 6.0 - 90.0, 90.0));
        assert_eq!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 61.0 }), None, "under fifty points inside the frame");
        assert!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 62.0 }).is_some());
        assert_eq!(scene_band(Rect { x: 0.0, y: 0.0, w: 12.0, h: 400.0 }), None, "a sidebar sliding shut is all frame");
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
    fn a_castle_window_dims_every_three_to_seven_seconds_one_at_a_time() {
        let starts = dim_starts_until(400.0);
        assert!(starts.len() >= 18);
        for w in starts.windows(2) {
            assert!((3.0..=7.0).contains(&(w[1] - w[0])), "{w:?}");
        }
        // Each dims for DIM_FOR and comes back.
        let s = starts[0];
        assert!(dimmed_window(s + DIM_FOR * 0.5).is_some() && dimmed_window(s + DIM_FOR + 0.01).is_none());
    }

    fn wash() -> &'static str {
        crate::effects::BUILTIN.iter().find(|b| b.name == "overworld").unwrap().wash
    }

    #[test]
    fn the_wash_declares_every_constant_mirrored_here_with_the_same_value() {
        let wash = wash();
        for line in [
            format!("constant float ART_PX = {ART_PX:?};"),
            format!("constant float SCENE_MAX_H = {SCENE_MAX_H:?};"),
            format!("constant float SCENE_TALL_ROOM = {SCENE_TALL_ROOM:?};"),
            format!("constant float SCENE_TALL_COLS = {SCENE_TALL_COLS:?};"),
            format!("constant float SOFT_BUFFER = {SOFT_BUFFER:?};"),
            format!("constant float SCENE_MIN_H = {SCENE_MIN_H:?};"),
            format!("constant float STAR_CLEAR = {STAR_CLEAR:?};"),
            format!("constant float DIM_CYCLE = {DIM_CYCLE:?};"),
            format!("constant float DIM_SLACK = {DIM_SLACK:?};"),
            format!("constant float DIM_FOR = {DIM_FOR:?};"),
            format!("constant int CASTLE_WINDOWS = {CASTLE_WINDOWS};"),
            format!("constant uint DIM_SALT = 0x{DIM_SALT:08X}u;"),
            format!("constant int FIREFLIES = {FIREFLIES};"),
            format!("constant float FIREFLY_PERIOD_MIN = {:?};", FIREFLY_PERIOD[0]),
            format!("constant float FIREFLY_PERIOD_MAX = {:?};", FIREFLY_PERIOD[1]),
            format!("constant float FIREFLY_FADE_IN = {FIREFLY_FADE_IN:?};"),
            format!("constant float FIREFLY_FADE_FROM = {FIREFLY_FADE_FROM:?};"),
            format!("constant float FIREFLY_FADE_TO = {FIREFLY_FADE_TO:?};"),
            format!("constant uint FIREFLY_SALT = 0x{FIREFLY_SALT:08X}u;"),
            format!("constant float TWINKLE_PERIOD_MIN = {:?};", TWINKLE_PERIOD[0]),
            format!("constant float TWINKLE_PERIOD_MAX = {:?};", TWINKLE_PERIOD[1]),
            format!("constant uint TWINKLE_SALT = 0x{TWINKLE_SALT:08X}u;"),
            format!("constant float TITLE_LEFT = {TITLE_LEFT:?};"),
            format!("constant int TITLE_CELL = {TITLE_CELL};"),
            format!("constant float TITLE_STAR_CHANCE = {TITLE_STAR_CHANCE:?};"),
            format!("constant uint TITLE_SALT = 0x{TITLE_SALT:08X}u;"),
            format!("constant float PLATE_PAD = {PLATE_PAD:?};"),
            format!("constant int CLOUD_W = {CLOUD_W};"),
            format!("constant int CLOUD_ROW = {CLOUD_ROW};"),
            format!("constant float CLOUD_SPEED = {CLOUD_SPEED:?};"),
            format!("constant float CLOUD_START = {CLOUD_START:?};"),
            format!("constant int CLOUD2_W = {CLOUD2_W};"),
            format!("constant float CLOUD2_RATE = {CLOUD2_RATE:?};"),
            format!("constant float CLOUD2_AHEAD = {CLOUD2_AHEAD:?};"),
            format!("constant int FRAME_PX = {FRAME_PX};"),
            format!("constant float GLINT_LEN = {GLINT_LEN:?};"),
            format!("constant float GLINT_SPEED = {GLINT_SPEED:?};"),
            format!("constant int ARROW_W = {ARROW_W};"),
            format!("constant int ARROW_IN = {ARROW_IN};"),
            format!("constant float ARROW_BOB = {ARROW_BOB:?};"),
            format!("constant float CURSOR_BOB = {CURSOR_BOB:?};"),
            format!("constant float PAPER = {PAPER:?};"),
            format!("constant float STAR_LIMIT = {STAR_LIMIT:?};"),
        ] {
            assert!(wash.contains(&line), "wash.metal lacks {line}");
        }
    }

    #[test]
    fn the_manifest_keeps_the_gap_the_frames_need_and_the_paper_the_wash_draws_through() {
        let m = crate::effects::manifest::parse(crate::effects::BUILTIN.iter().find(|b| b.name == "overworld").unwrap().manifest).unwrap();
        assert_eq!(m.gap, (2 * FRAME_PX + 1) as f32 * ART_PX, "two frames and an art pixel of ground between two panes");
        assert_eq!(m.paper_alpha, PAPER);
        assert_eq!(m.document_paper_alpha(), PAPER, "a document's paper is the terminal's: the night is behind both");
        assert!(!m.ink_borders, "tables, grids and videos keep the app's own lines");
        // A row's text starts 10 points in: with the padding it clears the frame, the cursor
        // (three art pixels) and the art pixel it bobs by.
        assert!(10.0 + m.sidebar_pad >= (FRAME_PX + 3 + 1) as f32 * ART_PX + 2.0);
    }

    /// A small Timo Purple window as the app reports it: the title bar with its words and icons,
    /// a 180 point sidebar whose names end at y 200, and two panes side by side, each `GAP` from
    /// its neighbours, the left one a terminal with its rows and its active tab.
    const W: u32 = 600;
    const H: u32 = 300;
    const SIDE: f32 = 180.0;
    const GAP: f32 = 15.0;
    const NAMES_END: f32 = 200.0;
    const WORDS: Rect = Rect { x: 80.0, y: 6.0, w: 120.0, h: 16.0 };
    const ICONS: Rect = Rect { x: 520.0, y: 4.0, w: 50.0, h: 20.0 };
    const TOP: f32 = crate::chrome::TITLE_BAR_H;
    const TAB: f32 = crate::pane_view::TAB_BAR_H;
    const LEFT: Rect = Rect { x: SIDE + GAP, y: TOP + GAP, w: 180.0, h: H as f32 - TOP - 2.0 * GAP };
    const RIGHT: Rect = Rect { x: SIDE + 2.0 * GAP + 180.0, y: TOP + GAP, w: 180.0, h: H as f32 - TOP - 2.0 * GAP };

    fn body(pane: Rect) -> Rect {
        Rect { x: pane.x, y: pane.y + TAB, w: pane.w, h: pane.h - TAB }
    }

    /// The window's blocks with the left pane's focus, and whether it works and the right waits.
    fn window(focus: f32, busy: bool, waiting: bool) -> Vec<crate::effects::blocks::Block> {
        use crate::effects::blocks::{blocks, Kind, Scene, Shape};
        let content = Rect { x: SIDE + GAP, y: TOP + GAP, w: W as f32 - SIDE - 2.0 * GAP, h: H as f32 - TOP - 2.0 * GAP };
        let panes = [(1, LEFT), (2, RIGHT)];
        let rows = body(LEFT);
        let shapes = [
            Shape { key: 1, rect: WORDS, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 2, rect: ICONS, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 7, rect: Rect { x: LEFT.x, y: LEFT.y, w: 100.0, h: TAB }, kind: Kind::ActiveTab, pitch: 0.0 },
            Shape { key: 8, rect: Rect { x: rows.x + 4.0, y: rows.y + 4.0, w: rows.w - 8.0, h: rows.h - 8.0 }, kind: Kind::Rows, pitch: 18.0 },
        ];
        let (busy, focus, waiting) = (if busy { vec![1] } else { vec![] }, [(1, focus)], if waiting { vec![2] } else { vec![] });
        let scene = Scene { gap: GAP, focus: &focus, waiting: &waiting, ..crate::effects::active::scene(content, GAP, None, Some(NAMES_END), &panes, &busy, &shapes, &[]) };
        blocks(&scene, false)
    }

    fn small_window() -> Vec<crate::effects::blocks::Block> {
        window(1.0, false, false)
    }

    /// Every pixel of the frame inside `r`, a window point a pixel.
    fn pixels(frame: &[crate::palette::Rgb], r: Rect) -> Vec<crate::palette::Rgb> {
        let (x0, y0, x1, y1) = (r.x as u32, r.y as u32, (r.x + r.w) as u32, (r.y + r.h) as u32);
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| frame[(y * W + x) as usize])).collect()
    }

    fn render(theme: &crate::theme::Theme, t: f32, blocks: &[crate::effects::blocks::Block]) -> Vec<crate::palette::Rgb> {
        crate::effects::bench::render_frame(wash(), theme, (W, H), t, blocks)
    }

    fn gpu() -> bool {
        metal::Device::system_default().is_some()
    }

    /// The points of the window where two frames differ.
    fn changed(a: &[crate::palette::Rgb], b: &[crate::palette::Rgb]) -> Vec<(f32, f32)> {
        (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| a[(y * W + x) as usize] != b[(y * W + x) as usize]).map(|(x, y)| (x as f32 + 0.5, y as f32 + 0.5)).collect()
    }

    /// The sidebar's inside, clear of its frame.
    fn sidebar_inside() -> Rect {
        let f = FRAME_PX as f32 * ART_PX;
        Rect { x: f, y: TOP + f, w: SIDE - 2.0 * f, h: H as f32 - TOP - 2.0 * f }
    }

    #[test]
    fn on_the_gpu_the_scene_stays_under_the_names_softened_until_the_dim_text_keeps_its_contrast() {
        if !gpu() {
            return;
        }
        use crate::effects::blocks::blocks;
        use crate::palette::contrast;
        for name in ["timo-purple", "arabia-solar", "navy", "peach-watercolor", "composition-notebook", "matrix", "material"] {
            let theme = crate::theme::builtin(name).unwrap();
            let (sb, dim) = (theme.chrome.sidebar, theme.chrome.text_dim);
            assert!(contrast(dim, sb) >= 4.5, "{name}");
            let content = Rect { x: SIDE + GAP, y: TOP + GAP, w: W as f32 - SIDE - 2.0 * GAP, h: H as f32 - TOP - 2.0 * GAP };
            let panes = [(1, content)];
            // A long file list: the names run to the sidebar's foot, over the whole scene.
            let scene = crate::effects::blocks::Scene { gap: GAP, ..crate::effects::active::scene(content, GAP, None, None, &panes, &[], &[], &[]) };
            for t in [0.0, 3.7, 21.3] {
                let frame = render(&theme, t, &blocks(&scene, false));
                let seen = pixels(&frame, sidebar_inside());
                assert!(seen.iter().any(|p| *p != sb), "{name} at {t}: the scene is still there under the names");
                let worst = seen.iter().map(|p| contrast(dim, *p)).fold(f64::INFINITY, f64::min);
                assert!(worst >= 4.5, "{name} at {t}: the dim text is {worst:.2}:1 on the softened scene");
            }
        }
    }

    /// A window whose sidebar's names end at `names_end`.
    fn window_with_names_to(names_end: f32) -> Vec<crate::effects::blocks::Block> {
        use crate::effects::blocks::{blocks, Kind, Scene, Shape};
        let content = Rect { x: SIDE + GAP, y: TOP + GAP, w: W as f32 - SIDE - 2.0 * GAP, h: H as f32 - TOP - 2.0 * GAP };
        let panes = [(1, content)];
        let shapes = [Shape { key: 1, rect: WORDS, kind: Kind::TitleGroup, pitch: 0.0 }, Shape { key: 2, rect: ICONS, kind: Kind::TitleGroup, pitch: 0.0 }];
        blocks(&Scene { gap: GAP, ..crate::effects::active::scene(content, GAP, None, Some(names_end), &panes, &[], &shapes, &[]) }, false)
    }

    #[test]
    fn on_the_gpu_the_scene_is_softened_only_under_the_names_and_a_buffer_below_them() {
        if !gpu() {
            return;
        }
        use crate::palette::contrast;
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let c = &theme.chrome;
        let band = scene_band(Rect { x: 0.0, y: TOP, w: SIDE, h: H as f32 - TOP }).unwrap();
        // A castle window is the focus colour at time 0 wherever the scene is at full colour.
        let lit = |names_end: f32| pixels(&render(&theme, 0.0, &window_with_names_to(names_end)), band).contains(&c.focus_ring);
        assert!(lit(band.y - 30.0), "names that end above the scene leave it at full colour");
        assert!(lit(band.y), "and ones that only reach its top");
        // Names into the scene: under them it is softened, so the dim text keeps its contrast
        // there; below them the scene comes back, and past the buffer it is untouched.
        for names_end in [band.y + 24.0, band.y + 90.0, band.y + band.h - 12.0] {
            let frame = render(&theme, 0.0, &window_with_names_to(names_end));
            let under = pixels(&frame, Rect { h: names_end - band.y, ..band });
            assert!(!under.contains(&c.focus_ring), "softened under the names at {names_end}");
            assert!(under.iter().all(|p| contrast(c.text_dim, *p) >= 4.5), "the dim text keeps its contrast under the names at {names_end}");
            assert!(under.iter().any(|p| *p != c.sidebar), "and the scene is still there at {names_end}");
            let past = names_end + SOFT_BUFFER;
            if past < band.y + band.h {
                let clear = Rect { y: past, h: band.y + band.h - past, ..band };
                let full = pixels(&render(&theme, 0.0, &window_with_names_to(band.y - 30.0)), clear);
                assert_eq!(pixels(&frame, clear), full, "past the buffer the scene is as it is with no names over it, at {names_end}");
            }
        }
        // Above the scene the sidebar is plain, whatever the names do.
        let frame = render(&theme, 3.7, &window_with_names_to(band.y + 60.0));
        let above = Rect { y: TOP + 6.0, h: band.y - TOP - 6.0, ..sidebar_inside() };
        assert!(pixels(&frame, above).iter().all(|p| *p == c.sidebar));
    }

    #[test]
    fn on_the_gpu_the_title_s_words_icons_and_traffic_lights_sit_on_plain_plates_in_a_living_sky() {
        if !gpu() {
            return;
        }
        use crate::palette::contrast;
        for name in ["timo-purple", "arabia-solar", "navy", "composition-notebook"] {
            let theme = crate::theme::builtin(name).unwrap();
            let c = &theme.chrome;
            let blocks = small_window();
            // Inside the frame: the words and the icons with their padding, and the lights.
            let inside = |x: f32, w: f32| Rect { x, y: 6.0, w, h: TOP - 12.0 };
            let plates = [
                inside(WORDS.x - PLATE_PAD + 3.0, WORDS.w + 2.0 * PLATE_PAD - 6.0),
                inside(ICONS.x - PLATE_PAD + 3.0, ICONS.w + 2.0 * PLATE_PAD - 6.0),
                inside(9.0, 52.0),
            ];
            let mut lively = false;
            for t in [0.0, 3.7, 9.4, 21.3, 64.0, 200.5] {
                let frame = render(&theme, t, &blocks);
                for plate in plates {
                    assert!(pixels(&frame, plate).iter().all(|p| *p == c.title_bar), "{name} at {t}: the plate at {plate:?} is the bar's plain colour");
                }
                // Between the words' plate and the icons' the sky goes on: a step darker than the
                // plates on every theme, so they are seen, with its stars.
                let sky = inside(WORDS.x + WORDS.w + PLATE_PAD, ICONS.x - PLATE_PAD - WORDS.x - WORDS.w - PLATE_PAD);
                let seen = pixels(&frame, sky);
                lively |= seen.iter().any(|p| contrast(*p, c.title_bar) > 1.3);
                // The sky's dither is a step darker than the bar over much of it, which is what
                // tells a plate from the sky around it.
                let dark = crate::palette::luminance(c.title_bar);
                let darker = seen.iter().filter(|p| contrast_of(crate::palette::luminance(**p), dark) > 1.03 && crate::palette::luminance(**p) < dark).count();
                assert!(darker * 4 >= seen.len(), "{name} at {t}: {darker} of {} sky pixels are darker than the plates", seen.len());
            }
            assert!(lively, "{name}: stars in the sky between the plates");
        }
        // The manifest moves the words and icons in from the frame by the plate's padding.
        let m = crate::effects::manifest::parse(crate::effects::BUILTIN.iter().find(|b| b.name == "overworld").unwrap().manifest).unwrap();
        assert_eq!(m.title_pad, PLATE_PAD);
    }

    /// The contrast of two luminances.
    fn contrast_of(a: f64, b: f64) -> f64 {
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn on_the_gpu_a_light_runs_along_the_title_bar_s_foot_now_and_then_and_never_in_the_still_frame() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let blocks = small_window();
        let foot = Rect { x: 3.0, y: TOP - 3.0, w: W as f32 - 6.0, h: 3.0 };
        let line = |t: f32| pixels(&render(&theme, t, &blocks), foot);
        let still = line(0.0);
        assert!(still.iter().all(|p| *p == still[0]), "the still frame's line is whole");
        assert!(line(3.0) == still, "none in the first cycle");
        // A second into the second cycle it is 160 art pixels along, lit and on the line only.
        let lit = line(7.0);
        assert!(lit != still && lit.iter().filter(|p| **p != still[0]).count() <= 12 * 9);
    }

    #[test]
    fn on_the_gpu_the_sidebar_is_boxed_in_the_panes_own_frame_and_the_title_bar_in_its_outer_line() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let frame = render(&theme, 0.0, &small_window());
        let at = |x: f32, y: f32| frame[(y as u32 * W + x as u32) as usize];
        // The unfocused pane's two lines, to compare with.
        let (line, dark) = (at(RIGHT.x - 4.5, RIGHT.y + 100.0), at(RIGHT.x - 1.5, RIGHT.y + 100.0));
        let mid = TOP + 100.0;
        assert_eq!((at(1.5, mid), at(4.5, mid)), (line, dark), "the sidebar's left edge");
        assert_eq!((at(SIDE - 1.5, mid), at(SIDE - 4.5, mid)), (line, dark), "its right edge");
        assert_eq!((at(60.0, H as f32 - 1.5), at(60.0, H as f32 - 4.5)), (line, dark), "its foot, over the scene");
        // The title bar's frame is the outer line alone, so the traffic lights have room.
        assert_eq!((at(300.0, 1.5), at(300.0, TOP - 1.5)), (line, line), "the title bar's top and foot");
        assert!(![line, dark].contains(&at(300.0, 4.5)) && ![line, dark].contains(&at(300.0, TOP - 4.5)), "one line, the sky inside it");
        // The notch: an outer corner is left to the ground, an inner one is the line's colour.
        assert!(at(1.5, TOP + 1.5) != line && at(1.5, TOP + 1.5) != dark);
        assert_eq!(at(4.5, TOP + 4.5), line);
    }

    #[test]
    fn on_the_gpu_each_tab_bar_carries_a_gem_lit_in_the_focus_colour_on_the_focused_pane() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let focus = theme.chrome.focus_ring;
        let frame = render(&theme, 0.0, &small_window());
        // Eleven to fifteen art pixels in from the bar's right, four rows from its third.
        let gem = |pane: Rect| pixels(&frame, Rect { x: pane.x + pane.w - 15.0 * ART_PX, y: pane.y + 3.0 * ART_PX, w: 4.0 * ART_PX, h: 4.0 * ART_PX });
        assert!(gem(LEFT).contains(&focus), "the focused pane's gem");
        assert!(!gem(RIGHT).contains(&focus), "another pane's is dull");
        let ledge = pixels(&frame, Rect { x: RIGHT.x + RIGHT.w - 30.0 * ART_PX, y: RIGHT.y + 3.0 * ART_PX, w: 4.0 * ART_PX, h: 4.0 * ART_PX });
        assert!(gem(RIGHT).iter().any(|p| !ledge.contains(p)), "but it is drawn");
    }

    #[test]
    fn on_the_gpu_each_pane_is_framed_in_the_gap_and_the_focused_one_in_the_focus_colour() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let focus = theme.chrome.focus_ring;
        let frame = render(&theme, 0.0, &small_window());
        let at = |x: f32, y: f32| frame[(y as u32 * W + x as u32) as usize];
        // Half way down each pane's left side: the outer line 3 to 6 points out, the inner one 0
        // to 3, and the ground beyond.
        let y = LEFT.y + 100.0;
        assert_eq!(at(LEFT.x - 4.5, y), focus, "the focused pane's outer line");
        assert_ne!(at(LEFT.x - 1.5, y), focus, "its inner line is the dark one");
        assert_ne!(at(RIGHT.x - 4.5, y), focus, "another pane's outer line is the frames' own colour");
        assert_ne!(at(RIGHT.x - 4.5, y), at(RIGHT.x - 7.5, y), "and is not the ground beyond it");
        assert_eq!(at(RIGHT.x - 1.5, y), at(LEFT.x - 1.5, y), "both inner lines are the same dark");
        // The notch: the outer line's corner is left to the ground, the inner one's is the line's.
        assert_ne!(at(LEFT.x - 4.5, LEFT.y - 4.5), focus);
        assert_eq!(at(LEFT.x - 1.5, LEFT.y - 1.5), focus);
        // Nothing of the frame is drawn inside a pane, and a pane half lit is between the two.
        for pane in [LEFT, RIGHT] {
            assert!(pixels(&frame, pane).iter().all(|p| *p != focus || pane == LEFT), "{pane:?}");
        }
        assert!(pixels(&frame, body(LEFT)).iter().all(|p| *p != focus), "the focus colour stays out of the body");
        let half = render(&theme, 0.0, &window(0.5, false, false));
        let mid = half[((y as u32) * W + (LEFT.x - 4.5) as u32) as usize];
        assert!(mid != focus && mid != at(RIGHT.x - 4.5, y), "a focus fade moves the line's colour: {mid:?}");
    }

    /// What reaches the screen from the wash's `p` behind a terminal's paper.
    fn papered(theme: &crate::theme::Theme, p: crate::palette::Rgb) -> crate::palette::Rgb {
        let bg = theme.terminal.background;
        let mix = |a: u8, b: u8| (a as f32 * PAPER + b as f32 * (1.0 - PAPER)).round() as u8;
        crate::palette::Rgb { r: mix(bg.r, p.r), g: mix(bg.g, p.g), b: mix(bg.b, p.b) }
    }

    #[test]
    fn on_the_gpu_nothing_behind_a_terminal_s_text_but_a_faint_star_lowers_its_contrast() {
        if !gpu() {
            return;
        }
        use crate::palette::contrast;
        for name in ["timo-purple", "arabia-solar", "navy", "peach-watercolor", "composition-notebook"] {
            let theme = crate::theme::builtin(name).unwrap();
            let (bg, fg, dim) = (theme.terminal.background, theme.terminal.foreground, theme.chrome.text_dim);
            let toward = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * STAR_LIMIT).round() as u8;
            let limit = crate::palette::Rgb { r: toward(bg.r, dim.r), g: toward(bg.g, dim.g), b: toward(bg.b, dim.b) };
            let (plain, floor) = (contrast(fg, bg), contrast(fg, limit));
            for t in [0.0, 5.3, 47.0] {
                let frame = render(&theme, t, &small_window());
                let seen: Vec<_> = pixels(&frame, body(LEFT)).into_iter().map(|p| papered(&theme, p)).collect();
                // By day the dunes at the foot are a touch toward the text's colour, costing it
                // under 7% of its contrast; only a star costs more.
                let stars = seen.iter().filter(|p| contrast(fg, **p) < plain * 0.93).count();
                assert!(seen.iter().all(|p| contrast(fg, *p) >= floor - 0.1), "{name} at {t}: a pixel under the star limit's {floor:.2}:1");
                // A star is one art pixel, nine points; a body this size has a handful.
                assert!(stars <= 9 * 12, "{name} at {t}: {stars} points lower the text's contrast");
                assert!(seen.iter().any(|p| *p != bg), "{name} at {t}: the night is drawn");
            }
        }
    }

    /// A title bar `w` points wide, its words from the traffic lights' end and its icons at its
    /// right, as the app lays them out.
    fn title_bar(w: f32) -> (Rect, [Rect; 2]) {
        (Rect { x: 0.0, y: 0.0, w, h: crate::chrome::TITLE_BAR_H }, [Rect { x: 80.0, y: 6.0, w: 300.0, h: 16.0 }, Rect { x: w - 70.0, y: 4.0, w: 56.0, h: 20.0 }])
    }

    #[test]
    fn the_title_bar_s_stars_run_its_whole_length_behind_its_words_too() {
        for w in [900.0, 1440.0, 2560.0] {
            let (bar, groups) = title_bar(w);
            let stars = title_stars(bar, &groups);
            assert!(stars.len() >= 12, "{} stars on a bar {w} points wide", stars.len());
            for ((i, j), _) in &stars {
                assert!(*i as f32 * ART_PX >= TITLE_LEFT && (2..=6).contains(j), "({i}, {j}) on a bar {w} wide: right of the traffic lights, inside the frame");
            }
            // Some in the clear, at full colour, and some behind the words, softened.
            assert!(stars.iter().any(|(_, clear)| *clear) && stars.iter().any(|(_, clear)| !*clear), "{w}");
        }
    }

    #[test]
    fn the_cloud_crosses_the_whole_bar_and_starts_part_of_the_way_along() {
        let w = 400;
        let from = (TITLE_LEFT / ART_PX).ceil() as i32;
        let seen: Vec<i32> = (0..4000).map(|k| cloud_at(w, k as f32 * 0.1)).collect();
        assert!(seen.iter().all(|x| *x >= from - CLOUD_W && *x < w), "it never leaves the bar's span");
        assert!(seen.contains(&(from - CLOUD_W)) && seen.contains(&(w - 1)), "it enters and leaves whole");
        assert!(cloud_at(w, 0.0) > from && cloud_at(w, 0.0) + CLOUD_W < w, "the still frame shows it whole");
    }

    #[test]
    fn on_the_gpu_the_title_bar_s_stars_are_where_the_mirror_puts_them() {
        if !gpu() {
            return;
        }
        use crate::effects::blocks::{Block, Kind};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let (w, h) = (2560u32, 28u32);
        let (bar, groups) = title_bar(w as f32);
        let block = |rect: Rect, kind: Kind| Block { rect, kind, edges: 0, seed: 1.0, value: 1.0, ordinal: 0.0, busy: 0.0, pitch: 0.0, focus: 0.0, waiting: 0.0 };
        let blocks = [block(bar, Kind::TitleBar), block(groups[0], Kind::TitleGroup), block(groups[1], Kind::TitleGroup)];
        let frame = crate::effects::bench::render_frame(wash(), &theme, (w, h), 0.0, &blocks);
        // What is lighter than the bar above its line, but for the cloud: the stars.
        let lum = crate::palette::luminance(theme.chrome.title_bar);
        let (cloud, cloud2) = (cloud_at(w as i32 / 3, 0.0), cloud_at(w as i32 / 3, CLOUD2_AHEAD));
        let clouded = move |i: i32, j: i32| (i >= cloud && i < cloud + CLOUD_W && j >= CLOUD_ROW && j < CLOUD_ROW + 3) || (i >= cloud2 && i < cloud2 + CLOUD2_W && j > CLOUD_ROW && j < CLOUD_ROW + 3);
        let mut drawn: Vec<(i32, i32)> = (6..h - 6)
            .flat_map(|y| (6..w - 6).map(move |x| (x, y)))
            .filter(|&(x, y)| crate::palette::luminance(frame[(y * w + x) as usize]) > lum)
            .map(|(x, y)| (x as i32 / 3, y as i32 / 3))
            .filter(|(i, j)| !clouded(*i, *j))
            .collect();
        drawn.sort();
        drawn.dedup();
        let stars: Vec<((i32, i32), bool)> = title_stars(bar, &groups).into_iter().filter(|((i, j), _)| !clouded(*i, *j)).collect();
        // Every star in the clear is drawn, and nothing is drawn that is not a star (one behind
        // the words may be softened all the way into the bar).
        for (star, clear) in &stars {
            assert!(!clear || drawn.contains(star), "{star:?}");
        }
        for cell in &drawn {
            assert!(stars.iter().any(|(star, _)| star == cell), "{cell:?} is no star");
        }
        // The frame's outer line runs the length of the bar's foot, between its corners.
        let line = frame[((h - 2) * w + 300) as usize];
        assert!((3..w - 3).all(|x| frame[((h - 2) * w + x) as usize] == line) && crate::palette::luminance(line) > lum);
    }

    #[test]
    fn on_the_gpu_a_selected_row_is_filled_to_the_sidebar_s_edge_with_the_cursor_in_its_padding() {
        if !gpu() {
            return;
        }
        use crate::effects::blocks::{blocks, HighlightState, Kind, Mark};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        // Mid-slide the sidebar shows 90 of its 180 points; the row keeps its full column.
        let content = Rect { x: 90.0, y: TOP, w: W as f32 - 90.0, h: H as f32 - TOP };
        let panes = [(1, content)];
        // The first row below the frame, level with the tab bar, which the fill would cover. Its
        // mark starts where its text does: past the padding and one level of indent.
        let row = Rect { x: 36.0, y: TOP + 8.0, w: SIDE - 36.0, h: 22.0 };
        let frame_of = |state: HighlightState, t: f32| {
            let strokes = [(Mark { key: 3, rect: row, kind: Kind::RowHighlight, state }, 1.0)];
            let scene = crate::effects::active::scene(content, 0.0, None, None, &panes, &[], &[], &strokes);
            render(&theme, t, &blocks(&scene, false))
        };
        let frame = frame_of(HighlightState::Selected, 0.0);
        let (fill, yellow, plain) = (theme.chrome.tab_active, theme.terminal.yellow, theme.chrome.sidebar);
        // The sidebar shows 90 points, its frame the last 6 of them.
        assert!(pixels(&frame, Rect { x: 36.0, y: row.y, w: 48.0, h: row.h }).iter().all(|p| *p == fill), "the row is filled inside the sidebar, plain under its text");
        // The cursor sits just inside the frame, in the padding before the row's text.
        let cursor = |f: &[crate::palette::Rgb], x: f32| pixels(f, Rect { x, y: row.y, w: 9.0, h: row.h }).iter().filter(|p| **p == yellow).count();
        assert_eq!(cursor(&frame, 6.0), 9 * 9, "the whole triangle, nine art pixels, at time 0");
        assert!(pixels(&frame, Rect { x: 6.0, y: row.y, w: 30.0, h: row.h }).iter().all(|p| *p == yellow || *p == plain), "nothing else before the mark");
        assert!(pixels(&frame, Rect { x: 18.0, y: row.y, w: 18.0, h: row.h }).iter().all(|p| *p == plain), "and clear of the text");
        assert!(pixels(&frame, Rect { x: 90.0, y: row.y, w: 90.0, h: row.h }).iter().all(|p| *p != fill && *p != yellow), "and nothing past the sidebar's edge, over the tab bar");
        // Half a second on it has bobbed one art pixel toward the row, still clear of the text.
        let bobbed = frame_of(HighlightState::Selected, CURSOR_BOB + 0.01);
        assert_eq!((cursor(&bobbed, 9.0), pixels(&bobbed, Rect { x: 6.0, y: row.y, w: 3.0, h: row.h }).contains(&yellow)), (9 * 9, false));
        assert!(pixels(&bobbed, Rect { x: 18.0, y: row.y, w: 18.0, h: row.h }).iter().all(|p| *p == plain));
        // A row only hovered has no cursor; one both selected and hovered keeps it.
        assert!(frame_of(HighlightState::Hovered, 0.0).iter().all(|p| *p != yellow));
        assert!(frame_of(HighlightState::Both, 0.0).contains(&yellow));
    }

    #[test]
    fn on_the_gpu_a_tab_too_narrow_for_its_padding_carries_no_lantern() {
        if !gpu() {
            return;
        }
        use crate::effects::blocks::{blocks, Kind, Shape};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let content = Rect { x: SIDE, y: TOP, w: W as f32 - SIDE, h: H as f32 - TOP };
        let panes = [(1, content)];
        let frame_with = |w: f32| {
            // Below the line along the doorway's top.
            let tab = Rect { x: SIDE, y: TOP, w, h: TAB };
            let shapes = [Shape { key: 7, rect: tab, kind: Kind::ActiveTab, pitch: 0.0 }];
            let scene = crate::effects::active::scene(content, 0.0, None, None, &panes, &[], &shapes, &[]);
            (pixels(&render(&theme, 0.0, &blocks(&scene, false)), Rect { y: TOP + ART_PX, h: TAB - ART_PX, ..tab }), theme.chrome.tab_active)
        };
        let (wide, fill) = frame_with(100.0);
        assert!(wide.iter().any(|p| *p != fill), "a tab with its padding carries its lantern");
        let (dragged, fill) = frame_with(24.0);
        assert!(dragged.iter().all(|p| *p == fill), "a dragged slot is plain");
    }

    #[test]
    fn on_the_gpu_a_working_pane_s_glint_never_leaves_its_frame_s_outer_line() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let mut seen = 0;
        for t in [0.0, 1.3, 9.9, 33.3, 71.0] {
            let diff = changed(&render(&theme, t, &window(1.0, false, false)), &render(&theme, t, &window(1.0, true, false)));
            assert!(!diff.is_empty() && diff.len() <= (GLINT_LEN as usize + 2) * 9, "{} points lit at {t}", diff.len());
            for p in &diff {
                assert_eq!(frame_ring(*p, LEFT), (FRAME_PX, false), "{p:?} at {t}");
            }
            seen += diff.len();
        }
        assert!(seen > 0);
        // The still frame under Reduce Motion: the glint at the frame's top left.
        let diff = changed(&render(&theme, 0.0, &window(1.0, false, false)), &render(&theme, 0.0, &window(1.0, true, false)));
        assert!(diff.iter().all(|p| p.0 < LEFT.x + 3.0 * ART_PX && p.1 < LEFT.y + GLINT_LEN * ART_PX), "{diff:?}");
    }

    #[test]
    fn on_the_gpu_a_document_s_night_scrolls_with_its_page_the_dusk_at_its_top_and_the_hills_at_its_end() {
        if !gpu() {
            return;
        }
        use crate::effects::blocks::{Block, Kind};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let bg = theme.terminal.background;
        let b = body(RIGHT);
        let (top, foot) = (Rect { h: 60.0, ..b }, Rect { y: b.y + b.h - 30.0, h: 30.0, ..b });
        // The right pane as a document whose page reaches `above` points over it and `below` under.
        let drawn = |above: f32, below: f32| {
            let mut blocks = window(1.0, false, false);
            let page = Rect { x: b.x, y: b.y - above, w: b.w, h: b.h + above + below };
            let at = blocks.iter().position(|k| k.kind == Kind::TitleGroup).unwrap();
            blocks.insert(at, Block { rect: page, kind: Kind::Page, edges: 0, seed: 9.0, value: 1.0, ordinal: 0.0, busy: 0.0, pitch: 0.0, focus: 0.0, waiting: 0.0 });
            let frame = render(&theme, 0.0, &blocks);
            let lit = |r: Rect| pixels(&frame, r).iter().any(|p| *p != bg);
            (lit(top), lit(foot), pixels(&frame, b).iter().any(|p| *p != bg))
        };
        assert_eq!(drawn(0.0, 0.0), (true, true, true), "a page that fits shows its dusk and its hills");
        assert_eq!(drawn(0.0, 900.0), (true, false, true), "at the top of a long page: the dusk, no hills yet");
        assert_eq!(drawn(900.0, 0.0), (false, true, true), "at its end: the hills, the dusk long gone");
        assert_eq!(drawn(900.0, 900.0), (false, false, false), "in between: the plain night");
        // Part of the way off the top, the dusk is part of the way out of the pane.
        assert_eq!(drawn(45.0, 900.0), (true, false, true));
        // A terminal beside it keeps its own night whatever the page does.
        let mut blocks = window(1.0, false, false);
        let before = pixels(&render(&theme, 0.0, &blocks), body(LEFT));
        blocks.push(Block { rect: Rect { x: b.x, y: b.y - 900.0, w: b.w, h: b.h + 1800.0 }, kind: Kind::Page, edges: 0, seed: 9.0, value: 1.0, ordinal: 0.0, busy: 0.0, pitch: 0.0, focus: 0.0, waiting: 0.0 });
        assert!(before == pixels(&render(&theme, 0.0, &blocks), body(LEFT)));
    }

    #[test]
    fn on_the_gpu_a_waiting_pane_shows_the_continue_arrow_bobbing_in_its_corner_and_no_other_does() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let b = body(RIGHT);
        let (foot, right) = (b.y + b.h, b.x + b.w);
        let arrow = |t: f32| changed(&render(&theme, t, &window(1.0, false, false)), &render(&theme, t, &window(1.0, false, true)));
        for t in [0.0, 0.9, 2.1] {
            let diff = arrow(t);
            assert_eq!(diff.len(), (5 + 3 + 1) * 9, "the whole arrow, nine art pixels, at {t}");
            for p in &diff {
                assert!(p.1 < foot - (ARROW_IN - 2) as f32 * ART_PX && p.1 > foot - (ARROW_IN + 4) as f32 * ART_PX, "{p:?} in the corner at {t}");
                assert!(p.0 < right - ARROW_IN as f32 * ART_PX && p.0 > right - (ARROW_IN + ARROW_W) as f32 * ART_PX, "{p:?} in the corner at {t}");
            }
        }
        // It bobs one art pixel down and back.
        let top = |t: f32| arrow(t).iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        assert_eq!((top(ARROW_BOB + 0.01) - top(0.0), top(2.0 * ARROW_BOB + 0.01) - top(0.0)), (ART_PX, 0.0));
        // The wash draws it as far past the terminal's yellow as its channels go, so behind the
        // tab's paper it reaches the screen as near that yellow as a paper at PAPER lets it.
        let (bg, yellow) = (theme.terminal.background, theme.terminal.yellow);
        let far = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) / (1.0 - PAPER)).clamp(0.0, 255.0).round() as u8;
        let seen = papered(&theme, crate::palette::Rgb { r: far(bg.r, yellow.r), g: far(bg.g, yellow.g), b: far(bg.b, yellow.b) });
        let frame = render(&theme, 0.0, &window(1.0, false, true));
        let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
        assert!(pixels(&frame, b).iter().map(|p| papered(&theme, *p)).any(|p| near(p.r, seen.r) && near(p.g, seen.g) && near(p.b, seen.b)), "{seen:?}");
    }

    #[test]
    fn typed_text_resolves_in_three_cuts_over_short4_as_blocks_by_night_and_as_gathering_sand_by_day() {
        let glyphs = crate::effects::BUILTIN.iter().find(|b| b.name == "overworld").unwrap().glyphs.unwrap();
        // 200 ms, Material's short4, in three stages with no blend between them.
        assert!(glyphs.contains("constant float FLASH = 0.2;") && glyphs.contains("int stage = int(age / FLASH * 3.0);"));
        // The theme's mode chooses the telling: the window declares ARKOS_LIGHT_GROUND.
        let (day, night) = glyphs.split_once("if (ARKOS_LIGHT_GROUND) {").and_then(|(_, rest)| rest.split_once("} else {")).expect("a branch for each");
        assert!(day.contains("GRAIN_FEW") && day.contains("GRAIN_MOST") && day.contains("GOLD"), "grains by day");
        assert!(night.contains("BLOCK_BIG") && night.contains("BLOCK_SMALL"), "blocks by night");
        // Nothing in either reads the clock but the stage, so a cut's picture holds still.
        assert!(!day.contains("*time") && !night.contains("*time"));
    }

    #[test]
    fn a_firefly_is_gone_before_it_leaves_the_scene() {
        for i in 0..FIREFLIES {
            for k in 0..200 {
                let (rise, alpha) = firefly(i, k as f32 * 0.1);
                assert!((0.0..=1.0).contains(&rise));
                if rise > 0.95 {
                    assert!(alpha < 0.05, "firefly {i} at {k}");
                }
            }
        }
    }
}
