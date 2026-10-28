//! The highlights' fades (spec 4.6): each mark an element reports fades in over
//! `HIGHLIGHT_ENTER`, and one that stops being reported fades out over `HIGHLIGHT_EXIT` from
//! where it was last laid out. Under Reduce Motion a stroke is at its target at once.

use super::blocks::Mark;
use std::time::Instant;

/// One highlight on its way in, at rest, or on its way out.
struct Stroke {
    mark: Mark,
    from: f32,
    to: f32,
    since: Instant,
}

impl Stroke {
    /// The fade now: rising on the motion module's `highlight_in`, falling on its `highlight_out`.
    fn value(&self, now: Instant) -> f32 {
        if self.to >= self.from {
            self.from + (self.to - self.from) * crate::motion::highlight_in(self.since, now)
        } else {
            self.to + (self.from - self.to) * crate::motion::highlight_out(self.since, now)
        }
    }

    fn retarget(&mut self, to: f32, now: Instant) {
        if (self.to - to).abs() > f32::EPSILON {
            self.from = self.value(now);
            self.to = to;
            self.since = now;
        }
    }
}

/// Every highlight still showing, keyed by its mark's key, in the order they first appeared.
#[derive(Default)]
pub struct Strokes {
    strokes: Vec<Stroke>,
}

impl Strokes {
    /// Takes this frame's `marks` and returns every stroke with its fade at `now`, and whether
    /// any is still moving.
    pub fn sync(&mut self, marks: &[Mark], now: Instant) -> (Vec<(Mark, f32)>, bool) {
        for mark in marks {
            match self.strokes.iter_mut().find(|s| s.mark.key == mark.key) {
                Some(stroke) => stroke.mark = *mark,
                None => self.strokes.push(Stroke { mark: *mark, from: 0.0, to: 0.0, since: now }),
            }
        }
        for stroke in &mut self.strokes {
            let want = if marks.iter().any(|m| m.key == stroke.mark.key) { 1.0 } else { 0.0 };
            stroke.retarget(want, now);
        }
        self.strokes.retain(|s| s.to > 0.0 || s.value(now) > 0.001);
        let moving = self.strokes.iter().any(|s| (s.value(now) - s.to).abs() > 0.001);
        (self.strokes.iter().map(|s| (s.mark, s.value(now))).collect(), moving)
    }
}

#[cfg(test)]
mod tests {
    use super::super::blocks::{HighlightState, Kind};
    use super::*;
    use crate::motion::{test_lock, HIGHLIGHT_ENTER, HIGHLIGHT_EXIT};
    use crate::tree::Rect;

    fn row(key: u64, y: f32) -> Mark {
        Mark { key, rect: Rect { x: 0.0, y, w: 240.0, h: 22.0 }, kind: Kind::RowHighlight, state: HighlightState::Hovered }
    }

    #[test]
    fn a_new_mark_fades_in_and_a_forgotten_one_fades_out_from_where_it_was() {
        let _lock = test_lock(false);
        let mut strokes = Strokes::default();
        let t0 = Instant::now();
        let (out, moving) = strokes.sync(&[row(1, 100.0)], t0);
        assert_eq!(out, vec![(row(1, 100.0), 0.0)]);
        assert!(moving);
        let (out, moving) = strokes.sync(&[row(1, 120.0)], t0 + HIGHLIGHT_ENTER / 2);
        assert!(out[0].1 > 0.0 && out[0].1 < 1.0, "half way in: {}", out[0].1);
        assert_eq!(out[0].0.rect.y, 120.0, "the stroke follows its element");
        assert!(moving);
        let t1 = t0 + HIGHLIGHT_ENTER;
        let (out, moving) = strokes.sync(&[row(1, 120.0)], t1);
        assert_eq!(out, vec![(row(1, 120.0), 1.0)]);
        assert!(!moving);
        // Forgotten: it fades out from the frame that stops reporting it, where it was last
        // laid out.
        let t2 = t1 + HIGHLIGHT_ENTER;
        assert_eq!(strokes.sync(&[], t2), (vec![(row(1, 120.0), 1.0)], true));
        let (out, moving) = strokes.sync(&[], t2 + HIGHLIGHT_EXIT / 2);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.rect.y, 120.0);
        assert!(out[0].1 < 1.0 && out[0].1 > 0.0, "half way out: {}", out[0].1);
        assert!(moving);
        let (out, moving) = strokes.sync(&[], t2 + HIGHLIGHT_EXIT);
        assert!(out.is_empty());
        assert!(!moving);
    }

    #[test]
    fn under_reduce_motion_a_stroke_is_at_its_target_at_once_and_nothing_moves() {
        let _lock = test_lock(true);
        let mut strokes = Strokes::default();
        let t0 = Instant::now();
        assert_eq!(strokes.sync(&[row(1, 100.0)], t0), (vec![(row(1, 100.0), 1.0)], false));
        assert_eq!(strokes.sync(&[], t0), (vec![], false));
    }
}
