//! A diagram's view in the rendered markdown (diagrams and math spec, section 2): its zoom steps
//! and its pan, kept inside the box the picture is fitted to. Pure.

/// The smallest and largest zoom, and the step between them.
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 3.0;
pub const STEP: f32 = 0.25;

/// The zoom one step in (`in_`) or out from `zoom`, within `MIN_ZOOM` and `MAX_ZOOM`, on the steps.
pub fn step(zoom: f32, in_: bool) -> f32 {
    let at = (zoom / STEP).round() * STEP;
    let next = if in_ { at + STEP } else { at - STEP };
    next.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The pan (the picture's offset in its box) kept so the picture covers the box when it is larger,
/// and is centred in it when it is smaller, on each axis. `size` is the box's, the picture is
/// `size * zoom`.
pub fn clamp(pan: (f32, f32), size: (f32, f32), zoom: f32) -> (f32, f32) {
    let axis = |p: f32, s: f32| {
        let room = s - s * zoom;
        if room >= 0.0 {
            room / 2.0
        } else {
            p.clamp(room, 0.0)
        }
    };
    (axis(pan.0, size.0), axis(pan.1, size.1))
}

/// The pan after zooming from `from` to `to` about the box's centre, kept in the box.
pub fn zoom_about(pan: (f32, f32), size: (f32, f32), from: f32, to: f32) -> (f32, f32) {
    let k = to / from;
    let c = (size.0 / 2.0, size.1 / 2.0);
    clamp((c.0 - (c.0 - pan.0) * k, c.1 - (c.1 - pan.1) * k), size, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_a_quarter_between_half_and_three_times() {
        assert_eq!(step(1.0, true), 1.25);
        assert_eq!(step(1.0, false), 0.75);
        assert_eq!(step(3.0, true), 3.0);
        assert_eq!(step(0.5, false), 0.5);
        assert_eq!(step(1.1, true), 1.25, "back onto the steps");
    }

    #[test]
    fn a_zoomed_picture_pans_within_its_box_and_a_small_one_is_centred() {
        let size = (400.0, 200.0);
        assert_eq!(clamp((50.0, -10.0), size, 2.0), (0.0, -10.0), "no gap at the left");
        assert_eq!(clamp((-900.0, -500.0), size, 2.0), (-400.0, -200.0), "no gap at the right or bottom");
        assert_eq!(clamp((0.0, 0.0), size, 0.5), (100.0, 50.0), "smaller than its box: centred");
        assert_eq!(zoom_about((0.0, 0.0), size, 1.0, 2.0), (-200.0, -100.0), "zooming in keeps the centre where it was");
        assert_eq!(zoom_about((-200.0, -100.0), size, 2.0, 1.0), (0.0, 0.0));
    }
}
