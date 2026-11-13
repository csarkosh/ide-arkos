//! What a session asks of the user (layouts spec, "The states"): the daemon's state and whether
//! the agent is blocked, with what only this window knows, whether the user has looked and what
//! they put away. Pure, so it is tested; `SessionManager` keeps the marks and the list and the
//! tab bar read `of`.
use protocol::ClaudeState;
use serde::{Deserialize, Serialize};

/// Where a session stands with the user, in the order a list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    /// The agent cannot go on until the user acts: a permission, a question, a plan to approve.
    NeedsYou,
    /// The agent finished its turn and the user has not looked since.
    Unread,
    /// The user looked and has not answered: still their move, no longer new.
    Unanswered,
    /// The user's last word is with the agent, which is working.
    Working,
    /// Not finished, but nothing is happening and nothing is asked of the user: a plain shell, an
    /// agent not yet asked anything, or one the user has left be.
    Quiet,
    /// Put off by the user until a time that has not come.
    Later,
    /// Put away by the user, or its shell has ended.
    Done,
}

impl Attention {
    /// Every kind, in the order a list shows its groups.
    pub const ALL: [Attention; 7] = [Attention::NeedsYou, Attention::Unread, Attention::Unanswered, Attention::Working, Attention::Quiet, Attention::Later, Attention::Done];

    /// The groups a list shows folded until the user opens them: what is quiet and what is done
    /// can wait; the rest is open.
    pub const FOLDED_AT_FIRST: [Attention; 2] = [Attention::Quiet, Attention::Done];

    /// The group's heading in a list.
    pub fn label(self) -> &'static str {
        match self {
            Attention::NeedsYou => "Needs you",
            Attention::Unread => "Unread",
            Attention::Unanswered => "Unanswered",
            Attention::Working => "Working",
            Attention::Quiet => "Quiet",
            Attention::Later => "Later",
            Attention::Done => "Done",
        }
    }

    /// Whether the session calls for the user now: what a count on the Dock icon counts, and
    /// what "the next session that needs you" goes to first.
    pub fn calls(self) -> bool {
        matches!(self, Attention::NeedsYou | Attention::Unread)
    }
}

/// What a window knows of the user's handling of one session, saved with its workspace. `seen`,
/// `done` and `later_until` are about the session's present turn: `turned` drops them when the
/// agent next finishes or is blocked, so a session put away comes back with something new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Marks {
    /// The user has looked since the session last began to wait.
    pub seen: bool,
    /// Put away by the user.
    pub done: bool,
    /// Put off until this time, in milliseconds since the Unix epoch.
    pub later_until: Option<u64>,
    /// Marked unread by the user, whatever the session is doing, until they next look at it.
    pub unread: bool,
    /// When the session's state last changed, in milliseconds since the Unix epoch; 0, unknown.
    pub since: u64,
}

impl Marks {
    /// The session began a new turn of waiting on the user at `now`: whatever the user did with
    /// the last one no longer applies. `shown` is whether the user is looking at it this moment
    /// (its tab shown and focused in the window in front), in which case it is seen at once.
    pub fn turned(&mut self, shown: bool, now: u64) {
        *self = Marks { seen: shown, done: false, later_until: None, unread: false, since: now };
    }

    /// The session's state changed at `now` in a way that is no new turn (it went to work).
    pub fn moved(&mut self, now: u64) {
        self.since = now;
    }
}

/// A section the user moves a session to from its row's "Move to" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTo {
    Unread,
    Unanswered,
    /// Put off until this time, in milliseconds since the Unix epoch.
    Later(u64),
    Done,
}

impl Marks {
    /// The marks with the session moved to `to`: each move sets what puts it there and clears
    /// what would hold it elsewhere.
    pub fn moved_to(self, to: MoveTo) -> Marks {
        let since = self.since;
        match to {
            MoveTo::Unread => Marks { seen: false, done: false, later_until: None, unread: true, since },
            MoveTo::Unanswered => Marks { seen: true, done: false, later_until: None, unread: false, since },
            MoveTo::Later(until) => Marks { seen: true, done: false, later_until: Some(until), unread: false, since },
            MoveTo::Done => Marks { seen: true, done: true, later_until: None, unread: false, since },
        }
    }
}

/// The sections a session can be moved to, by name, in the list's order: each that the move
/// would really put it in (one waiting on nothing cannot be put off, a working one is never
/// unanswered, a blocked one stays where it is), less the one it is in. `tomorrow` is the time
/// "tomorrow at 9" names.
pub fn move_targets(state: Option<ClaudeState>, blocked: bool, alive: bool, marks: &Marks, now: u64, tomorrow: u64) -> Vec<(&'static str, MoveTo)> {
    let here = of(state, blocked, alive, marks, now);
    [
        ("Unread", MoveTo::Unread, Attention::Unread),
        ("Unanswered", MoveTo::Unanswered, Attention::Unanswered),
        ("Later: in an hour", MoveTo::Later(now + 3_600_000), Attention::Later),
        ("Later: tomorrow at 9", MoveTo::Later(tomorrow), Attention::Later),
        ("Done", MoveTo::Done, Attention::Done),
    ]
    .into_iter()
    .filter(|(_, to, lands)| *lands != here && of(state, blocked, alive, &marks.moved_to(*to), now) == *lands)
    .map(|(label, to, _)| (label, to))
    .collect()
}

/// Where a session stands. `state` is the daemon's (None: never reported), `blocked` its agent's
/// block, `alive` whether its shell still runs.
pub fn of(state: Option<ClaudeState>, blocked: bool, alive: bool, marks: &Marks, now: u64) -> Attention {
    if !alive {
        return Attention::Done;
    }
    // A block overrides whatever the user put away: nothing moves until they act.
    if blocked {
        return Attention::NeedsYou;
    }
    // Marked unread by the user: back as new, from wherever it was.
    if marks.unread {
        return Attention::Unread;
    }
    if marks.done {
        return Attention::Done;
    }
    match state {
        Some(ClaudeState::Waiting) if marks.later_until.is_some_and(|t| t > now) => Attention::Later,
        // A later time that has come is a session back in the list as new.
        Some(ClaudeState::Waiting) if marks.seen && marks.later_until.is_none() => Attention::Unanswered,
        Some(ClaudeState::Waiting) => Attention::Unread,
        Some(ClaudeState::Working) => Attention::Working,
        Some(ClaudeState::Idle) | None => Attention::Quiet,
    }
}

/// How long ago `since` was, as a list says it: "now" under a minute, then "4 min", "2 h", "3 d".
/// Nothing for a time that is unknown.
pub fn age(since: u64, now: u64) -> String {
    if since == 0 {
        return String::new();
    }
    let secs = now.saturating_sub(since) / 1000;
    match secs {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    #[test]
    fn a_session_s_attention_follows_its_state_its_block_and_what_the_user_did_with_it() {
        let fresh = Marks::default();
        let seen = Marks { seen: true, ..fresh };
        let at = |state, blocked, alive, marks: &Marks| of(state, blocked, alive, marks, NOW);
        assert_eq!(at(Some(ClaudeState::Waiting), true, true, &fresh), Attention::NeedsYou);
        assert_eq!(at(Some(ClaudeState::Waiting), false, true, &fresh), Attention::Unread);
        assert_eq!(at(Some(ClaudeState::Waiting), false, true, &seen), Attention::Unanswered);
        assert_eq!(at(Some(ClaudeState::Working), false, true, &seen), Attention::Working);
        assert_eq!(at(Some(ClaudeState::Idle), false, true, &fresh), Attention::Quiet);
        assert_eq!(at(None, false, true, &fresh), Attention::Quiet, "a plain shell");
        assert_eq!(at(Some(ClaudeState::Working), false, false, &fresh), Attention::Done, "its shell ended");
        // Looking at a block does not end it: it needs an answer, not a glance.
        assert_eq!(at(Some(ClaudeState::Waiting), true, true, &seen), Attention::NeedsYou);
    }

    #[test]
    fn done_and_later_put_a_session_away_until_it_has_something_new() {
        let done = Marks { seen: true, done: true, ..Marks::default() };
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &done, NOW), Attention::Done);
        assert_eq!(of(Some(ClaudeState::Working), false, true, &done, NOW), Attention::Done, "a working session put away stays away while it works");
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &done, NOW), Attention::NeedsYou, "a block brings it back");
        let later = Marks { seen: true, later_until: Some(NOW + 60_000), ..Marks::default() };
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &later, NOW), Attention::Later);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &later, NOW + 60_000), Attention::Unread, "its time came: back as new");
        // The next turn drops both marks, and is seen at once when the user is looking at it.
        let mut m = done;
        m.turned(false, NOW);
        assert_eq!(m, Marks { seen: false, done: false, later_until: None, unread: false, since: NOW });
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unread);
        m.turned(true, NOW + 5);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unanswered);
        m.moved(NOW + 9);
        assert_eq!((m.seen, m.since), (true, NOW + 9), "going to work keeps the marks and moves the time");
    }

    #[test]
    fn a_session_the_user_marks_unread_is_unread_whatever_it_is_doing_but_a_block_still_comes_first() {
        let marked = Marks { unread: true, ..Marks::default() };
        for state in [Some(ClaudeState::Idle), Some(ClaudeState::Working), Some(ClaudeState::Waiting), None] {
            assert_eq!(of(state, false, true, &marked, NOW), Attention::Unread, "{state:?}");
        }
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &Marks { done: true, ..marked }, NOW), Attention::Unread, "out of Done");
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &marked, NOW), Attention::NeedsYou);
        assert_eq!(of(Some(ClaudeState::Idle), false, false, &marked, NOW), Attention::Done, "an ended shell is done");
        let mut next = marked;
        next.turned(false, NOW);
        assert!(!next.unread, "a new turn starts from what the agent says");
    }

    #[test]
    fn move_to_offers_the_sections_the_move_would_really_put_a_session_in() {
        let names = |state, blocked, marks: &Marks| move_targets(state, blocked, true, marks, NOW, NOW + 9).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        let fresh = Marks::default();
        // Finished and unread: anywhere but where it is.
        assert_eq!(names(Some(ClaudeState::Waiting), false, &fresh), ["Unanswered", "Later: in an hour", "Later: tomorrow at 9", "Done"]);
        assert_eq!(names(Some(ClaudeState::Waiting), false, &Marks { seen: true, ..fresh }), ["Unread", "Later: in an hour", "Later: tomorrow at 9", "Done"]);
        // Working or idle: nothing waits to be put off or answered.
        assert_eq!(names(Some(ClaudeState::Working), false, &fresh), ["Unread", "Done"]);
        assert_eq!(names(None, false, &fresh), ["Unread", "Done"]);
        assert_eq!(names(Some(ClaudeState::Idle), false, &Marks { done: true, ..fresh }), ["Unread"], "out of Done, to the one section an idle session can go");
        // Blocked: it needs the user, and no move changes that.
        assert!(names(Some(ClaudeState::Waiting), true, &fresh).is_empty());
        assert!(move_targets(Some(ClaudeState::Waiting), false, false, &fresh, NOW, NOW).is_empty(), "an ended shell is done");
        // Each move lands where it says.
        let m = fresh.moved_to(MoveTo::Later(NOW + 5));
        assert_eq!((of(Some(ClaudeState::Waiting), false, true, &m, NOW), of(Some(ClaudeState::Waiting), false, true, &m, NOW + 5)), (Attention::Later, Attention::Unread));
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &Marks { done: true, ..fresh }.moved_to(MoveTo::Unanswered), NOW), Attention::Unanswered);
        assert_eq!(fresh.moved_to(MoveTo::Done).since, fresh.since, "a move keeps the time of the session's last change");
    }

    #[test]
    fn the_groups_are_listed_from_what_calls_for_the_user_to_what_is_put_away() {
        assert_eq!(Attention::ALL.map(Attention::label), ["Needs you", "Unread", "Unanswered", "Working", "Quiet", "Later", "Done"]);
        assert_eq!(Attention::ALL.map(Attention::calls), [true, true, false, false, false, false, false]);
        let mut sorted = Attention::ALL;
        sorted.reverse();
        sorted.sort();
        assert_eq!(sorted, Attention::ALL);
    }

    #[test]
    fn an_age_is_said_in_the_largest_unit_that_fits() {
        let ago = |secs: u64| age(NOW * 1000 - secs * 1000, NOW * 1000);
        assert_eq!([ago(0), ago(59), ago(60), ago(3599), ago(3600), ago(86_399), ago(86_400), ago(400_000)], ["now", "now", "1 min", "59 min", "1 h", "23 h", "1 d", "4 d"]);
        assert_eq!(age(0, NOW), "", "unknown");
    }
}
