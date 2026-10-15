//! Smooth scrolling as a pure function of time (spec 3). Position is in lines of history:
//! 0.0 is the live bottom, larger is further up. Non-integer values are drawn as a fractional
//! pixel offset by the element.

pub struct ScrollAnimator {
    pub current: f32,
    pub target: f32,
}

/// Time constant of the ease-out, in seconds. 0.06 feels like a trackpad glide without lag.
const TAU: f32 = 0.06;
const SETTLE: f32 = 0.005;

impl ScrollAnimator {
    pub fn new() -> Self {
        Self { current: 0.0, target: 0.0 }
    }

    pub fn scroll_by(&mut self, delta_lines: f32, max: f32) {
        self.target = (self.target + delta_lines).clamp(0.0, max.max(0.0));
    }

    /// Moves by `delta_lines` with no glide: for a trackpad, whose pixel steps and momentum are
    /// the glide already, so easing them would only lag behind the finger.
    pub fn scroll_direct(&mut self, delta_lines: f32, max: f32) {
        self.scroll_by(delta_lines, max);
        self.current = self.target;
    }

    pub fn jump_to(&mut self, lines: f32) {
        self.target = lines.max(0.0);
        self.current = self.target;
    }

    pub fn clamp_max(&mut self, max: f32) {
        let max = max.max(0.0);
        self.target = self.target.min(max);
        self.current = self.current.min(max);
    }

    /// Advances by `dt_seconds`. Returns true while another frame is needed.
    pub fn tick(&mut self, dt_seconds: f32) -> bool {
        let diff = self.target - self.current;
        if diff.abs() < SETTLE {
            self.current = self.target;
            return false;
        }
        let alpha = 1.0 - (-dt_seconds / TAU).exp();
        self.current += diff * alpha;
        true
    }

    // Not called in milestone 1; kept for milestone 2, where tabs show new output below a scrolled view.
    #[allow(dead_code)]
    pub fn is_at_bottom(&self) -> bool {
        self.current == 0.0 && self.target == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_toward_target_and_settles() {
        let mut s = ScrollAnimator::new();
        s.scroll_by(10.0, 100.0);
        assert!(s.tick(0.016));
        assert!(s.current > 0.0 && s.current < 10.0, "moved part way: {}", s.current);
        for _ in 0..60 {
            s.tick(0.016);
        }
        assert!(!s.tick(0.016), "should have settled");
        assert_eq!(s.current, 10.0);
    }

    #[test]
    fn landing_point_is_independent_of_frame_rate() {
        let mut a = ScrollAnimator::new();
        let mut b = ScrollAnimator::new();
        a.scroll_by(25.0, 100.0);
        b.scroll_by(25.0, 100.0);
        for _ in 0..100 { a.tick(0.016); }
        for _ in 0..10 { b.tick(0.16); }
        assert!((a.current - 25.0).abs() < 0.001);
        assert!((b.current - 25.0).abs() < 0.001);
    }

    #[test]
    fn a_direct_step_lands_at_once_and_needs_no_frame() {
        let mut s = ScrollAnimator::new();
        s.scroll_by(10.0, 100.0);
        s.tick(0.016);
        // A trackpad step mid-glide lands the whole way, glide included.
        s.scroll_direct(0.4, 100.0);
        assert_eq!(s.current, 10.4);
        assert_eq!(s.target, 10.4);
        assert!(!s.tick(0.016));
        s.scroll_direct(-20.0, 100.0);
        assert_eq!(s.current, 0.0, "clamped at the live bottom");
    }

    #[test]
    fn clamps_to_history_and_bottom() {
        let mut s = ScrollAnimator::new();
        s.scroll_by(500.0, 42.0);
        assert_eq!(s.target, 42.0);
        s.scroll_by(-1000.0, 42.0);
        assert_eq!(s.target, 0.0);
    }

    #[test]
    fn shrinking_history_clamps_current_position() {
        let mut s = ScrollAnimator::new();
        s.jump_to(50.0);
        s.clamp_max(20.0);
        assert_eq!(s.current, 20.0);
        assert_eq!(s.target, 20.0);
    }

    #[test]
    fn at_bottom_only_when_settled_at_zero() {
        let mut s = ScrollAnimator::new();
        assert!(s.is_at_bottom());
        s.scroll_by(3.0, 10.0);
        assert!(!s.is_at_bottom());
        s.jump_to(0.0);
        assert!(s.is_at_bottom());
    }
}
