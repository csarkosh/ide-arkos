//! What a session asks of the user (layouts spec, "The states"): the daemon's state and whether
//! the agent is blocked, with what only this window knows, whether the user has looked and what
//! they put away. Pure, so it is tested; `SessionManager` keeps the marks and the list and the
//! tab bar read `of`.
use protocol::ClaudeState;
use serde::{Deserialize, Serialize};

/// Where a session stands with the user, in the order a list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Attention {
    /// The agent cannot go on until the user acts: a permission, a question, a plan to approve.
    NeedsYou,
    /// The agent finished its turn and the user has not looked since.
    Unread,
    /// The user looked and has not answered: still their move, no longer new.
    Unanswered,
    /// The user's last word is with the agent, which is working.
    Working,
    /// Nothing pending: a plain shell, or an agent not yet asked anything.
    Idle,
    /// Put off by the user until a time that has not come.
    Later,
    /// Put away by the user, or its shell has ended.
    Done,
}

impl Attention {
    /// Every kind, in the order a list shows its groups.
    pub const ALL: [Attention; 7] = [Attention::NeedsYou, Attention::Unread, Attention::Unanswered, Attention::Working, Attention::Idle, Attention::Later, Attention::Done];

    /// The group's heading in a list.
    pub fn label(self) -> &'static str {
        match self {
            Attention::NeedsYou => "Needs you",
            Attention::Unread => "Unread",
            Attention::Unanswered => "Unanswered",
            Attention::Working => "Working",
            Attention::Idle => "Idle",
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
    /// When the session's state last changed, in milliseconds since the Unix epoch; 0, unknown.
    pub since: u64,
}

impl Marks {
    /// The session began a new turn of waiting on the user at `now`: whatever the user did with
    /// the last one no longer applies. `shown` is whether the user is looking at it this moment
    /// (its tab shown and focused in the window in front), in which case it is seen at once.
    pub fn turned(&mut self, shown: bool, now: u64) {
        *self = Marks { seen: shown, done: false, later_until: None, since: now };
    }

    /// The session's state changed at `now` in a way that is no new turn (it went to work).
    pub fn moved(&mut self, now: u64) {
        self.since = now;
    }
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
    if marks.done {
        return Attention::Done;
    }
    match state {
        Some(ClaudeState::Waiting) if marks.later_until.is_some_and(|t| t > now) => Attention::Later,
        // A later time that has come is a session back in the list as new.
        Some(ClaudeState::Waiting) if marks.seen && marks.later_until.is_none() => Attention::Unanswered,
        Some(ClaudeState::Waiting) => Attention::Unread,
        Some(ClaudeState::Working) => Attention::Working,
        Some(ClaudeState::Idle) | None => Attention::Idle,
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
        assert_eq!(at(Some(ClaudeState::Idle), false, true, &fresh), Attention::Idle);
        assert_eq!(at(None, false, true, &fresh), Attention::Idle, "a plain shell");
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
        assert_eq!(m, Marks { seen: false, done: false, later_until: None, since: NOW });
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unread);
        m.turned(true, NOW + 5);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unanswered);
        m.moved(NOW + 9);
        assert_eq!((m.seen, m.since), (true, NOW + 9), "going to work keeps the marks and moves the time");
    }

    #[test]
    fn the_groups_are_listed_from_what_calls_for_the_user_to_what_is_put_away() {
        assert_eq!(Attention::ALL.map(Attention::label), ["Needs you", "Unread", "Unanswered", "Working", "Idle", "Later", "Done"]);
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
