//! An image zoomed over its markdown document (markdown view group spec, section 2): fitted to
//! the pane first, then zoomed further by a click, Option+click or the bar's buttons, and panned
//! by two fingers on the trackpad, the wheel or a drag. Pure, so it is tested; the window keeps
//! one `View` while an image is zoomed.

/// The zooms a click steps through, as multiples of the fitted size.
pub const STEPS: [f32; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

/// How far a mouse wheel's line moves the picture, in points.
pub const LINE: f32 = 40.0;

/// The zoomed image's state: its zoom over the fitted size, where it lies in its box, a drag
/// under way, and a single click waiting to see whether it is the first of a double click.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct View {
    /// 1 is the picture fitted to the box.
    pub zoom: f32,
    /// The picture's top left in its box, while it is larger than the box on that axis.
    pub pan: (f32, f32),
    /// A press: where it was, the pan then, and whether it has moved enough to be a drag.
    pub drag: Option<((f32, f32), (f32, f32), bool)>,
    /// A click waiting for the double-click time to pass: its number, so a later click or a
    /// double click supersedes it.
    pub pending: u64,
}

impl View {
    pub fn new() -> View {
        View { zoom: 1.0, ..View::default() }
    }
}

/// The picture's size fitted into a box, kept to its proportions, larger or smaller.
pub fn fitted(natural: (f32, f32), bx: (f32, f32)) -> (f32, f32) {
    if natural.0 <= 0.0 || natural.1 <= 0.0 || bx.0 <= 0.0 || bx.1 <= 0.0 {
        return (bx.0.max(0.0), bx.1.max(0.0));
    }
    let k = (bx.0 / natural.0).min(bx.1 / natural.1);
    (natural.0 * k, natural.1 * k)
}

/// The next zoom in (`in_`) or out from `zoom`, on the steps, within the first and last.
pub fn step(zoom: f32, in_: bool) -> f32 {
    if in_ {
        STEPS.iter().copied().find(|s| *s > zoom + 1e-3).unwrap_or(STEPS[STEPS.len() - 1])
    } else {
        STEPS.iter().rev().copied().find(|s| *s < zoom - 1e-3).unwrap_or(STEPS[0])
    }
}

/// The pan kept so a picture larger than its box covers it and one smaller is centred, on each
/// axis. `pic` is the picture's size as drawn.
pub fn clamp(pan: (f32, f32), bx: (f32, f32), pic: (f32, f32)) -> (f32, f32) {
    let axis = |p: f32, b: f32, s: f32| {
        let room = b - s;
        if room >= 0.0 {
            room / 2.0
        } else {
            p.clamp(room, 0.0)
        }
    };
    (axis(pan.0, bx.0, pic.0), axis(pan.1, bx.1, pic.1))
}

/// The pan after zooming from `from` to `to` with the point `at` (in the box) staying where it
/// is, kept in the box. `fit` is the fitted size.
pub fn zoom_about(pan: (f32, f32), at: (f32, f32), fit: (f32, f32), bx: (f32, f32), from: f32, to: f32) -> (f32, f32) {
    // From where the picture lies now (centred when it fitted), not a stale pan.
    let pan = clamp(pan, bx, (fit.0 * from, fit.1 * from));
    let k = to / from;
    clamp((at.0 - (at.0 - pan.0) * k, at.1 - (at.1 - pan.1) * k), bx, (fit.0 * to, fit.1 * to))
}

/// The scale the picture is drawn at against its own pixels, as the bar says it: "250%".
pub fn percent(natural: (f32, f32), fit: (f32, f32), zoom: f32) -> String {
    if natural.0 <= 0.0 {
        return String::new();
    }
    format!("{}%", (fit.0 * zoom / natural.0 * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fitted_to_its_box_up_or_down_keeping_its_shape() {
        assert_eq!(fitted((400.0, 200.0), (800.0, 800.0)), (800.0, 400.0), "smaller: scaled up to the width");
        assert_eq!(fitted((4000.0, 1000.0), (800.0, 600.0)), (800.0, 200.0));
        assert_eq!(fitted((1000.0, 2000.0), (800.0, 600.0)), (300.0, 600.0), "tall: to the height");
    }

    #[test]
    fn a_click_steps_the_zoom_in_and_option_click_out_within_fit_and_eight_times() {
        assert_eq!(step(1.0, true), 1.5);
        assert_eq!(step(1.5, true), 2.0);
        assert_eq!(step(8.0, true), 8.0, "no further");
        assert_eq!(step(2.0, false), 1.5);
        assert_eq!(step(1.0, false), 1.0, "never smaller than fitted");
        assert_eq!(step(2.5, false), 2.0, "back onto the steps");
    }

    #[test]
    fn zooming_about_a_point_keeps_it_under_the_pointer_and_the_picture_in_its_box() {
        let (fit, bx) = ((800.0, 400.0), (800.0, 600.0));
        // Fitted, the picture is centred: its top at 100.
        assert_eq!(clamp((0.0, 0.0), bx, fit), (0.0, 100.0));
        // Doubled about the box's centre: the centre stays put.
        let pan = zoom_about((0.0, 0.0), (400.0, 300.0), fit, bx, 1.0, 2.0);
        assert_eq!(pan, (-400.0, -100.0));
        // About a point near the left: that point stays where it was.
        let at = (100.0, 300.0);
        let pan = zoom_about((0.0, 0.0), at, fit, bx, 1.0, 2.0);
        let before = ((at.0 - 0.0) / 1.0, (at.1 - 100.0) / 1.0);
        let after = ((at.0 - pan.0) / 2.0, (at.1 - pan.1) / 2.0);
        assert_eq!(before, after, "the same spot of the picture under the pointer");
        // Back to fitted: centred again.
        assert_eq!(zoom_about(pan, at, fit, bx, 2.0, 1.0), (0.0, 100.0));
    }

    #[test]
    fn a_pan_never_shows_a_gap_and_a_small_axis_stays_centred() {
        let bx = (800.0, 600.0);
        let pic = (1600.0, 500.0);
        assert_eq!(clamp((100.0, 0.0), bx, pic), (0.0, 50.0), "no gap at the left; the short axis centred");
        assert_eq!(clamp((-5000.0, 0.0), bx, pic), (-800.0, 50.0), "no gap at the right");
    }

    #[test]
    fn the_bar_says_the_scale_against_the_picture_s_own_pixels() {
        assert_eq!(percent((400.0, 200.0), (800.0, 400.0), 1.0), "200%");
        assert_eq!(percent((4000.0, 1000.0), (800.0, 200.0), 2.0), "40%");
        assert_eq!(percent((0.0, 0.0), (800.0, 400.0), 1.0), "");
    }
}
