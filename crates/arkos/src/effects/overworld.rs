//! The built-in `overworld` effect's layout and timing, mirrored from its wash shader
//! (`effects/overworld/wash.metal`) so they can be tested without a GPU. The constants and the
//! arithmetic here are the shader's own; a change to one is made to both.

pub use super::hash::hash01;
use crate::tree::Rect;

/// An art pixel's side, in points. The wash draws at `scale = 1.0`, and every art pixel is
/// snapped to whole points from its band's origin, so its edges land on window pixels.
pub const ART_PX: f32 = 3.0;
/// The tallest the scene is drawn, in points: 46 art pixels fit under it.
pub const SCENE_MAX_H: f32 = 140.0;
/// Below this much room at the foot of the sidebar there is no scene at all.
pub const SCENE_MIN_H: f32 = 50.0;
/// How far, in points, a title bar star keeps from each of the bar's word and icon groups.
pub const STAR_CLEAR: f32 = 12.0;

/// Seconds from one castle window's dimming to the next, give or take half of `DIM_SLACK`, so
/// two are 10 to 20 seconds apart. Each dims for `DIM_FOR` and comes back.
pub const DIM_CYCLE: f32 = 15.0;
pub const DIM_SLACK: f32 = 5.0;
pub const DIM_FOR: f32 = 0.8;
/// The castle's lit windows, which the shader draws in this order.
pub const CASTLE_WINDOWS: usize = 5;
/// A salt that gives the castle windows' dimming hashes of its own.
pub const DIM_SALT: u32 = 0x85EB_CA6B;

/// How many fireflies rise through the scene.
pub const FIREFLIES: usize = 3;
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
pub const TWINKLE_PERIOD: [f32; 2] = [4.0, 9.0];
/// A salt that gives the stars' twinkles hashes of their own.
pub const TWINKLE_SALT: u32 = 0x1656_67B1;

/// The band at the bottom of the sidebar's foot the scene is drawn in: as wide as the foot, at
/// most `SCENE_MAX_H` tall in whole art pixels, anchored to the foot's bottom, so a shorter foot
/// cuts the scene from the top. `None` when the foot has under `SCENE_MIN_H` of room.
pub fn scene_band(foot: Rect) -> Option<Rect> {
    if foot.h < SCENE_MIN_H {
        return None;
    }
    let h = (foot.h.min(SCENE_MAX_H) / ART_PX).floor() * ART_PX;
    Some(Rect { x: foot.x, y: foot.y + foot.h - h, w: foot.w, h })
}

/// Whether a title bar star may be drawn at `p`: outside every group grown by `STAR_CLEAR` on
/// every side.
pub fn star_clear(p: (f32, f32), groups: &[Rect]) -> bool {
    !groups.iter().any(|g| {
        p.0 >= g.x - STAR_CLEAR && p.0 <= g.x + g.w + STAR_CLEAR && p.1 >= g.y - STAR_CLEAR && p.1 <= g.y + g.h + STAR_CLEAR
    })
}

/// The title bar's stars: none before `TITLE_LEFT`, the traffic lights' end; one in some of the
/// cells of `TITLE_CELL` art pixels, on a row from 1 to 7 of the bar, and only in the
/// `TITLE_END_CELLS` cells at each end of a free stretch, so a wide bar still has only a few.
pub const TITLE_LEFT: f32 = 80.0;
pub const TITLE_CELL: i32 = 12;
pub const TITLE_STAR_CHANCE: f32 = 0.5;
pub const TITLE_END_CELLS: i32 = 4;
/// A salt that gives the title bar's stars hashes of their own.
pub const TITLE_SALT: u32 = 0x9E37_79B9;

/// The free stretch of the title bar `bar` a star at window x `x` lies in, its left and right
/// in window points: from `TITLE_LEFT`, or the nearest group before it grown by `STAR_CLEAR`, to
/// the bar's end, or the nearest group after it grown by `STAR_CLEAR`.
pub fn free_stretch(x: f32, bar: Rect, groups: &[Rect]) -> (f32, f32) {
    groups.iter().fold((TITLE_LEFT, bar.x + bar.w), |(lo, hi), g| {
        let (left, right) = (g.x - STAR_CLEAR, g.x + g.w + STAR_CLEAR);
        (if right <= x { lo.max(right) } else { lo }, if left >= x + ART_PX { hi.min(left) } else { hi })
    })
}

/// The art pixels, counted from the bar's origin, of the stars drawn in a title bar laid out at
/// `bar` with its word and icon `groups`, as the shader draws them: in each cell that wins its
/// chance and lies among the `TITLE_END_CELLS` at either end of its free stretch, at a seeded
/// place, when the whole star keeps clear of every group.
pub fn title_stars(bar: Rect, groups: &[Rect]) -> Vec<(i32, i32)> {
    if groups.is_empty() {
        return Vec::new();
    }
    let cells = (bar.w / ART_PX).floor() as i32 / TITLE_CELL + 1;
    (0..cells)
        .filter_map(|cell| {
            let id = cell as u32 ^ TITLE_SALT;
            let star = (cell * TITLE_CELL + 2 + (hash01(id.wrapping_add(1)) * (TITLE_CELL - 4) as f32) as i32, 1 + (hash01(id.wrapping_add(2)) * 7.0) as i32);
            if hash01(id) >= TITLE_STAR_CHANCE {
                return None;
            }
            let o = (bar.x + star.0 as f32 * ART_PX, bar.y + star.1 as f32 * ART_PX);
            let corners = [o, (o.0 + ART_PX, o.1), (o.0, o.1 + ART_PX), (o.0 + ART_PX, o.1 + ART_PX)];
            let (lo, hi) = free_stretch(o.0, bar, groups);
            let cw = ART_PX * TITLE_CELL as f32;
            let (first, last) = (((lo - bar.x) / cw).floor() as i32, ((hi - bar.x) / cw).floor() as i32);
            let near_end = cell - first < TITLE_END_CELLS || last - cell < TITLE_END_CELLS;
            let clear = near_end && o.0 >= TITLE_LEFT && o.0 + ART_PX <= bar.x + bar.w && corners.iter().all(|c| star_clear(*c, groups));
            clear.then_some(star)
        })
        .collect()
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

/// A star's brightness at `t` seconds, 0.5 to 1: a slow cosine with a period of its own, so
/// every star is at full brightness at the clock's zero.
pub fn twinkle(seed: f32, t: f32) -> f32 {
    let period = TWINKLE_PERIOD[0] + (TWINKLE_PERIOD[1] - TWINKLE_PERIOD[0]) * hash01(seed as u32 ^ TWINKLE_SALT);
    0.75 + 0.25 * (std::f32::consts::TAU * t / period).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scene_fills_the_foot_up_to_its_full_height_from_the_bottom() {
        let band = scene_band(Rect { x: 0.0, y: 400.0, w: 240.0, h: 300.0 }).unwrap();
        assert_eq!(band, Rect { x: 0.0, y: 562.0, w: 240.0, h: 138.0 }, "46 art pixels, the most under 140 points");
    }

    #[test]
    fn a_shorter_foot_cuts_the_scene_from_the_top_in_whole_art_pixels() {
        let band = scene_band(Rect { x: 0.0, y: 600.0, w: 240.0, h: 91.0 }).unwrap();
        assert_eq!((band.y, band.h), (691.0 - 90.0, 90.0));
    }

    #[test]
    fn the_scene_never_reaches_above_the_foot_so_no_name_sits_on_art() {
        for h in 0..400 {
            let foot = Rect { x: 0.0, y: 300.0, w: 200.0, h: h as f32 * 0.5 };
            if let Some(band) = scene_band(foot) {
                assert!(band.y >= foot.y && band.y + band.h == foot.y + foot.h, "{foot:?} {band:?}");
            }
        }
    }

    #[test]
    fn under_fifty_points_of_room_there_is_no_scene() {
        assert_eq!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 49.0 }), None);
        assert!(scene_band(Rect { x: 0.0, y: 0.0, w: 240.0, h: 50.0 }).is_some());
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
    fn a_castle_window_dims_every_ten_to_twenty_seconds_one_at_a_time() {
        let starts = dim_starts_until(400.0);
        assert!(starts.len() >= 18);
        for w in starts.windows(2) {
            assert!((10.0..=20.0).contains(&(w[1] - w[0])), "{w:?}");
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
            format!("constant int TITLE_END_CELLS = {TITLE_END_CELLS};"),
            format!("constant uint TITLE_SALT = 0x{TITLE_SALT:08X}u;"),
        ] {
            assert!(wash.contains(&line), "wash.metal lacks {line}");
        }
    }

    /// A small Timo Purple window as the app reports it: the title bar with its words and icons,
    /// a 180 point sidebar whose names end at y 120, and one pane with its active tab.
    const W: u32 = 400;
    const H: u32 = 300;
    const SIDE: f32 = 180.0;
    const NAMES_END: f32 = 120.0;
    const WORDS: Rect = Rect { x: 80.0, y: 6.0, w: 120.0, h: 16.0 };
    const ICONS: Rect = Rect { x: 340.0, y: 4.0, w: 50.0, h: 20.0 };

    fn small_window() -> Vec<crate::effects::blocks::Block> {
        use crate::effects::blocks::{blocks, Kind, Shape};
        let top = crate::chrome::TITLE_BAR_H;
        let content = Rect { x: SIDE, y: top, w: W as f32 - SIDE, h: H as f32 - top };
        let panes = [(1, content)];
        let shapes = [
            Shape { key: 1, rect: WORDS, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 2, rect: ICONS, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 7, rect: Rect { x: SIDE, y: top, w: 100.0, h: crate::pane_view::TAB_BAR_H }, kind: Kind::ActiveTab, pitch: 0.0 },
        ];
        let scene = crate::effects::active::scene(content, 0.0, None, Some(NAMES_END), &panes, &[], &shapes, &[]);
        blocks(&scene, false)
    }

    /// Every pixel of the frame inside `r`, a window point a pixel.
    fn pixels(frame: &[crate::palette::Rgb], r: Rect) -> Vec<crate::palette::Rgb> {
        let (x0, y0, x1, y1) = (r.x as u32, r.y as u32, (r.x + r.w) as u32, (r.y + r.h) as u32);
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| frame[(y * W + x) as usize])).collect()
    }

    #[test]
    fn on_the_gpu_the_names_title_and_tab_bodies_sit_on_plain_colour_and_only_the_foot_has_art() {
        if metal::Device::system_default().is_none() {
            return;
        }
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let c = &theme.chrome;
        let blocks = small_window();
        let top = crate::chrome::TITLE_BAR_H;
        let tab = crate::pane_view::TAB_BAR_H;
        let band = scene_band(Rect { x: 0.0, y: NAMES_END, w: SIDE, h: H as f32 - NAMES_END }).unwrap();
        for t in [0.0, 3.7, 21.3] {
            let frame = crate::effects::bench::render_frame(wash(), &theme, (W, H), t, &blocks);
            let body = Rect { x: SIDE, y: top + tab, w: W as f32 - SIDE, h: H as f32 - top - tab };
            assert!(pixels(&frame, body).iter().all(|p| *p == c.window_bg), "a tab body is plain window_bg at {t}");
            let names = Rect { x: 0.0, y: top, w: SIDE, h: band.y - top };
            assert!(pixels(&frame, names).iter().all(|p| *p == c.sidebar), "the sidebar above the scene is plain at {t}");
            for g in [WORDS, ICONS] {
                assert!(pixels(&frame, g).iter().all(|p| *p == c.title_bar), "the title bar is plain under {g:?} at {t}");
            }
            assert!(pixels(&frame, band).iter().any(|p| *p != c.sidebar), "the scene is drawn in the foot at {t}");
        }
    }

    /// A title bar `w` points wide, its words from the traffic lights' end and its icons at its
    /// right, as the app lays them out.
    fn title_bar(w: f32) -> (Rect, [Rect; 2]) {
        (Rect { x: 0.0, y: 0.0, w, h: crate::chrome::TITLE_BAR_H }, [Rect { x: 80.0, y: 6.0, w: 300.0, h: 16.0 }, Rect { x: w - 70.0, y: 4.0, w: 56.0, h: 20.0 }])
    }

    #[test]
    fn a_wide_title_bar_still_has_only_a_few_stars() {
        for w in [900.0, 1440.0, 2560.0, 3840.0, 6000.0] {
            let (bar, groups) = title_bar(w);
            let n = title_stars(bar, &groups).len();
            assert!((1..=8).contains(&n), "{n} stars on a bar {w} points wide");
        }
    }

    #[test]
    fn on_the_gpu_the_title_bar_s_stars_are_where_the_mirror_puts_them() {
        if metal::Device::system_default().is_none() {
            return;
        }
        use crate::effects::blocks::{Block, Kind};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let (w, h) = (2560u32, 28u32);
        let (bar, groups) = title_bar(w as f32);
        let block = |rect: Rect, kind: Kind| Block { rect, kind, edges: 0, seed: 1.0, value: 1.0, ordinal: 0.0, busy: 0.0, pitch: 0.0 };
        let blocks = [block(bar, Kind::TitleBar), block(groups[0], Kind::TitleGroup), block(groups[1], Kind::TitleGroup)];
        let frame = crate::effects::bench::render_frame(wash(), &theme, (w, h), 0.0, &blocks);
        let mut drawn: Vec<(i32, i32)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| frame[(y * w + x) as usize] != theme.chrome.title_bar).map(|(x, y)| (x as i32 / 3, y as i32 / 3)).collect();
        drawn.sort();
        drawn.dedup();
        assert_eq!(drawn, title_stars(bar, &groups));
    }

    #[test]
    fn on_the_gpu_a_selected_row_s_fill_ends_at_the_sidebar_s_edge_while_it_slides_out() {
        if metal::Device::system_default().is_none() {
            return;
        }
        use crate::effects::blocks::{blocks, HighlightState, Kind, Mark};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let top = crate::chrome::TITLE_BAR_H;
        // Mid-slide the sidebar shows 90 of its 180 points; the row keeps its full column.
        let content = Rect { x: 90.0, y: top, w: W as f32 - 90.0, h: H as f32 - top };
        let panes = [(1, content)];
        // The first row, level with the tab bar, whose colour the fill would cover.
        let row = Rect { x: 0.0, y: top + 2.0, w: SIDE, h: 22.0 };
        let strokes = [(Mark { key: 3, rect: row, kind: Kind::RowHighlight, state: HighlightState::Selected }, 1.0)];
        let scene = crate::effects::active::scene(content, 0.0, None, None, &panes, &[], &[], &strokes);
        let frame = crate::effects::bench::render_frame(wash(), &theme, (W, H), 0.0, &blocks(&scene, false));
        assert_ne!(theme.chrome.tab_active, theme.chrome.tab_bar);
        assert!(pixels(&frame, Rect { x: 0.0, y: row.y, w: 90.0, h: row.h }).iter().all(|p| *p == theme.chrome.tab_active), "the row is filled inside the sidebar");
        assert!(pixels(&frame, Rect { x: 90.0, y: row.y, w: 90.0, h: row.h }).iter().all(|p| *p == theme.chrome.tab_bar), "and not past its edge, over the tab bar");
    }

    #[test]
    fn on_the_gpu_a_tab_too_narrow_for_its_padding_carries_no_lantern() {
        if metal::Device::system_default().is_none() {
            return;
        }
        use crate::effects::blocks::{blocks, Kind, Shape};
        let theme = crate::theme::builtin("timo-purple").unwrap();
        let top = crate::chrome::TITLE_BAR_H;
        let content = Rect { x: SIDE, y: top, w: W as f32 - SIDE, h: H as f32 - top };
        let panes = [(1, content)];
        let frame_with = |w: f32| {
            let tab = Rect { x: SIDE, y: top, w, h: crate::pane_view::TAB_BAR_H };
            let shapes = [Shape { key: 7, rect: tab, kind: Kind::ActiveTab, pitch: 0.0 }];
            let scene = crate::effects::active::scene(content, 0.0, None, None, &panes, &[], &shapes, &[]);
            (pixels(&crate::effects::bench::render_frame(wash(), &theme, (W, H), 0.0, &blocks(&scene, false)), tab), theme.chrome.tab_active)
        };
        let (wide, fill) = frame_with(100.0);
        assert!(wide.iter().any(|p| *p != fill), "a tab with its padding carries its lantern");
        let (dragged, fill) = frame_with(24.0);
        assert!(dragged.iter().all(|p| *p == fill), "a dragged slot is plain");
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
