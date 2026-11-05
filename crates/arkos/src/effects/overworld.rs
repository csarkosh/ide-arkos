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
