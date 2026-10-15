//! When a staged update may apply itself (auto-update spec, section 6): the window has been
//! quiet for `idle_after`, nothing transient is open, nothing is in flight, the daemon is there.
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Chrome {
    pub palette: bool,
    pub menu: bool,
    pub prompt: bool,
    pub find: bool,
    pub drag: bool,
}

impl Chrome {
    pub fn any(self) -> bool {
        self.palette || self.menu || self.prompt || self.find || self.drag
    }
}

pub fn may_apply(now: Instant, last_input: Instant, idle_after: Duration, chrome: Chrome, in_flight: bool, connected: bool) -> bool {
    if chrome.any() || in_flight || !connected {
        return false;
    }
    now.saturating_duration_since(last_input) >= idle_after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_only_after_a_quiet_spell_with_nothing_open() {
        let t0 = Instant::now();
        let after = Duration::from_secs(30);
        let quiet = Chrome::default();
        assert!(!may_apply(t0 + Duration::from_secs(29), t0, after, quiet, false, true), "not yet");
        assert!(may_apply(t0 + Duration::from_secs(30), t0, after, quiet, false, true));
        assert!(may_apply(t0 + Duration::from_secs(300), t0, after, quiet, false, true));
        for open in [Chrome { palette: true, ..quiet }, Chrome { menu: true, ..quiet }, Chrome { prompt: true, ..quiet }, Chrome { find: true, ..quiet }, Chrome { drag: true, ..quiet }] {
            assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, open, false, true), "{open:?}");
        }
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, true, true), "an update already running");
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, false, false), "offline");
        assert!(may_apply(t0, t0 + Duration::from_secs(5), Duration::ZERO, quiet, false, true), "a last input in the future (clock skew) counts as idle only when idle_after is zero");
    }
}
