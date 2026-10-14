//! Tabs closed in the last minute, for Cmd+Shift+T (spec 4). Closing a terminal tab detaches its
//! session at once but kills it only after `CLOSE_GRACE`, so reopening within the grace brings
//! back the same shell with its history. Pure bookkeeping; the window sends the requests.

use crate::tree::PaneId;
use protocol::SessionId;
use std::time::{Duration, Instant};

/// How long a closed tab's session survives, waiting to be reopened.
pub const CLOSE_GRACE: Duration = Duration::from_secs(60);
/// How often the window ends sessions whose grace has run out.
pub const CLOSE_TICK: Duration = Duration::from_secs(5);
/// Closed tabs kept for reopening; closing more ends the oldest at once.
pub const MAX_CLOSED: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedTab {
    pub session: SessionId,
    /// The pane it was closed from, where it reopens if that pane still exists.
    pub pane: PaneId,
    pub closed_at: Instant,
}

impl ClosedTab {
    pub fn in_grace(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.closed_at) < CLOSE_GRACE
    }
}

/// Closed tabs, oldest first.
#[derive(Debug, Default)]
pub struct ClosedStack {
    entries: Vec<ClosedTab>,
}

impl ClosedStack {
    /// Records a closed tab. Returns the sessions pushed out past `MAX_CLOSED`, to kill now.
    pub fn push(&mut self, tab: ClosedTab) -> Vec<SessionId> {
        self.entries.push(tab);
        let over = self.entries.len().saturating_sub(MAX_CLOSED);
        self.entries.drain(..over).map(|e| e.session).collect()
    }

    /// Takes the most recently closed tab, in grace or not; the caller checks `in_grace`.
    pub fn pop(&mut self) -> Option<ClosedTab> {
        self.entries.pop()
    }

    /// Removes and returns every session whose grace has run out, to kill.
    pub fn expired(&mut self, now: Instant) -> Vec<SessionId> {
        let (gone, kept): (Vec<ClosedTab>, Vec<ClosedTab>) = self.entries.iter().partition(|e| !e.in_grace(now));
        self.entries = kept;
        gone.into_iter().map(|e| e.session).collect()
    }

    /// Removes and returns every session, for quitting: nothing could reopen them afterwards.
    pub fn drain_all(&mut self) -> Vec<SessionId> {
        self.entries.drain(..).map(|e| e.session).collect()
    }

    pub fn contains(&self, session: SessionId) -> bool {
        self.entries.iter().any(|e| e.session == session)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(session: u64, pane: PaneId, at: Instant) -> ClosedTab {
        ClosedTab { session: SessionId(session), pane, closed_at: at }
    }

    #[test]
    fn a_tab_reopens_only_within_the_grace() {
        let t0 = Instant::now();
        let tab = closed(1, 4, t0);
        assert!(tab.in_grace(t0));
        assert!(tab.in_grace(t0 + CLOSE_GRACE - Duration::from_millis(1)));
        assert!(!tab.in_grace(t0 + CLOSE_GRACE));
        // A clock read before the close (never in practice) counts as in grace.
        assert!(closed(1, 4, t0 + Duration::from_secs(1)).in_grace(t0));
    }

    #[test]
    fn pop_takes_the_newest_first() {
        let t0 = Instant::now();
        let mut s = ClosedStack::default();
        assert!(s.push(closed(1, 4, t0)).is_empty());
        assert!(s.push(closed(2, 5, t0 + Duration::from_secs(1))).is_empty());
        assert!(s.contains(SessionId(1)) && s.contains(SessionId(2)));
        assert_eq!(s.pop(), Some(closed(2, 5, t0 + Duration::from_secs(1))));
        assert!(!s.contains(SessionId(2)));
        assert_eq!(s.pop(), Some(closed(1, 4, t0)));
        assert_eq!(s.pop(), None);
    }

    #[test]
    fn expired_takes_only_sessions_past_the_grace() {
        let t0 = Instant::now();
        let mut s = ClosedStack::default();
        s.push(closed(1, 4, t0));
        s.push(closed(2, 4, t0 + Duration::from_secs(30)));
        assert!(s.expired(t0 + Duration::from_secs(59)).is_empty());
        assert_eq!(s.expired(t0 + CLOSE_GRACE), vec![SessionId(1)]);
        assert_eq!(s.len(), 1);
        assert_eq!(s.expired(t0 + Duration::from_secs(120)), vec![SessionId(2)]);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn pushing_past_the_limit_evicts_the_oldest() {
        let t0 = Instant::now();
        let mut s = ClosedStack::default();
        for i in 0..MAX_CLOSED as u64 {
            assert!(s.push(closed(i, 1, t0)).is_empty());
        }
        assert_eq!(s.push(closed(100, 1, t0)), vec![SessionId(0)]);
        assert_eq!(s.push(closed(101, 1, t0)), vec![SessionId(1)]);
        assert_eq!(s.len(), MAX_CLOSED);
        assert!(!s.contains(SessionId(0)) && s.contains(SessionId(100)));
        assert_eq!(s.drain_all().len(), MAX_CLOSED);
        assert_eq!(s.len(), 0);
    }
}
