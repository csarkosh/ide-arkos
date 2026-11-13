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
    /// Just started by the user, with nothing happening in it yet: for `NEW_FOR`, after which
    /// one still untouched is quiet.
    New,
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
    pub const ALL: [Attention; 8] = [Attention::NeedsYou, Attention::Unread, Attention::New, Attention::Unanswered, Attention::Working, Attention::Quiet, Attention::Later, Attention::Done];

    /// The groups a list shows folded until the user opens them: what is quiet and what is done
    /// can wait; the rest is open.
    pub const FOLDED_AT_FIRST: [Attention; 2] = [Attention::Quiet, Attention::Done];

    /// The group's heading in a list.
    pub fn label(self) -> &'static str {
        match self {
            Attention::NeedsYou => "Needs you",
            Attention::Unread => "Unread",
            Attention::New => "New",
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
    /// Started by the user and new until this time, while nothing has happened in it.
    pub new_until: Option<u64>,
    /// Moved by the user into this section, where it stays until its agent next does something.
    pub placed: Option<Attention>,
}

/// How long a session the user started stays new with nothing happening in it, in milliseconds:
/// long enough to type its first prompt or come back to it, after which it is quiet.
pub const NEW_FOR: u64 = 10 * 60 * 1000;

impl Marks {
    /// The session began a new turn of waiting on the user at `now`: whatever the user did with
    /// the last one no longer applies. `shown` is whether the user is looking at it this moment
    /// (its tab shown and focused in the window in front), in which case it is seen at once.
    pub fn turned(&mut self, shown: bool, now: u64) {
        *self = Marks { seen: shown, since: now, ..Marks::default() };
    }

    /// The session's state changed at `now` in a way that is no new turn (it went to work). It is
    /// no longer new.
    pub fn moved(&mut self, now: u64) {
        self.since = now;
        self.new_until = None;
        self.placed = None;
    }

    /// The user started the session at `now`: new for `NEW_FOR`, unless something happens first.
    pub fn started(&mut self, now: u64) {
        *self = Marks { since: now, new_until: Some(now + NEW_FOR), ..Marks::default() };
    }
}

/// A section the user moves a session to from its row's "Move to" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTo {
    /// To that section, whatever the session is doing. Later takes a time (`Later`) instead.
    To(Attention),
    /// Put off until this time, in milliseconds since the Unix epoch.
    Later(u64),
}

impl Marks {
    /// The marks with the session moved to `to`: each move sets what puts it there and clears
    /// what would hold it elsewhere.
    /// Unread lasts until the user next looks at the session, Later until its time, and every
    /// other section until the agent next does something; Done until it next finishes or needs
    /// the user.
    pub fn moved_to(self, to: MoveTo) -> Marks {
        let kept = Marks { since: self.since, ..Marks::default() };
        match to {
            MoveTo::To(Attention::Unread) => Marks { unread: true, ..kept },
            MoveTo::To(Attention::Done) => Marks { seen: true, done: true, ..kept },
            MoveTo::Later(until) => Marks { seen: true, later_until: Some(until), ..kept },
            MoveTo::To(other) => Marks { seen: true, placed: Some(other), ..kept },
        }
    }
}

/// The sections a session can be moved to, by name, in the list's order: every one but the one
/// it is in, whatever it is doing, less Later, which the row menu's "Remind me in" puts it in
/// with a time. None for a shell that has ended.
pub fn move_targets(state: Option<ClaudeState>, blocked: bool, alive: bool, marks: &Marks, now: u64) -> Vec<(&'static str, MoveTo)> {
    if !alive {
        return Vec::new();
    }
    let here = of(state, blocked, alive, marks, now);
    Attention::ALL.into_iter().filter(|k| *k != here && *k != Attention::Later).map(|k| (k.label(), MoveTo::To(k))).collect()
}

/// Where a session stands. `state` is the daemon's (None: never reported), `blocked` its agent's
/// block, `alive` whether its shell still runs.
pub fn of(state: Option<ClaudeState>, blocked: bool, alive: bool, marks: &Marks, now: u64) -> Attention {
    if !alive {
        return Attention::Done;
    }
    // Where the user put it stands, a block included, until the agent's next turn clears it
    // (`Marks::turned`, `Marks::moved`): a new block, a new answer, a new prompt.
    if let Some(placed) = marks.placed {
        return placed;
    }
    if marks.later_until.is_some_and(|t| t > now) {
        return Attention::Later;
    }
    // Marked unread by the user: back as new, from wherever it was.
    if marks.unread {
        return Attention::Unread;
    }
    if marks.done {
        return Attention::Done;
    }
    if blocked {
        return Attention::NeedsYou;
    }
    match state {
        // A later time that has come is a session back in the list as new.
        Some(ClaudeState::Waiting) if marks.seen && marks.later_until.is_none() => Attention::Unanswered,
        Some(ClaudeState::Waiting) => Attention::Unread,
        Some(ClaudeState::Working) => Attention::Working,
        // Just started, nothing in it yet: new for a while, then quiet.
        Some(ClaudeState::Idle) | None if marks.new_until.is_some_and(|t| t > now) => Attention::New,
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
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &done, NOW), Attention::Done, "put away while blocked: the user's word stands");
        let mut blocked_later = done;
        blocked_later.turned(false, NOW);
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &blocked_later, NOW), Attention::NeedsYou, "a block that comes after brings it back");
        let later = Marks { seen: true, later_until: Some(NOW + 60_000), ..Marks::default() };
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &later, NOW), Attention::Later);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &later, NOW + 60_000), Attention::Unread, "its time came: back as new");
        // The next turn drops both marks, and is seen at once when the user is looking at it.
        let mut m = done;
        m.turned(false, NOW);
        assert_eq!(m, Marks { seen: false, done: false, later_until: None, unread: false, since: NOW, new_until: None, placed: None });
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unread);
        m.turned(true, NOW + 5);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &m, NOW), Attention::Unanswered);
        m.moved(NOW + 9);
        assert_eq!((m.seen, m.since), (true, NOW + 9), "going to work keeps the marks and moves the time");
    }

    #[test]
    fn a_session_the_user_marks_unread_is_unread_whatever_it_is_doing_until_a_block_comes_after() {
        let marked = Marks { unread: true, ..Marks::default() };
        for state in [Some(ClaudeState::Idle), Some(ClaudeState::Working), Some(ClaudeState::Waiting), None] {
            assert_eq!(of(state, false, true, &marked, NOW), Attention::Unread, "{state:?}");
        }
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &Marks { done: true, ..marked }, NOW), Attention::Unread, "out of Done");
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &marked, NOW), Attention::Unread, "marked while blocked: the user's word stands");
        let mut then_blocked = marked;
        then_blocked.turned(false, NOW);
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &then_blocked, NOW), Attention::NeedsYou, "a block that comes after needs them");
        assert_eq!(of(Some(ClaudeState::Idle), false, false, &marked, NOW), Attention::Done, "an ended shell is done");
        let mut next = marked;
        next.turned(false, NOW);
        assert!(!next.unread, "a new turn starts from what the agent says");
    }

    #[test]
    fn move_to_offers_every_section_but_the_one_a_session_is_in_and_each_move_lands_there() {
        let names = |state, blocked, marks: &Marks| move_targets(state, blocked, true, marks, NOW).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        let fresh = Marks::default();
        // Later is not among them: "Remind me in" puts a session there with a time.
        let all = ["Needs you", "Unread", "New", "Unanswered", "Working", "Quiet", "Done"];
        let but = |here: &str| all.iter().copied().filter(|n| !n.starts_with(here)).collect::<Vec<_>>();
        assert_eq!(names(None, false, &fresh), but("Quiet"), "an idle session can go anywhere but Quiet");
        assert_eq!(names(Some(ClaudeState::Working), false, &fresh), but("Working"));
        assert_eq!(names(Some(ClaudeState::Waiting), false, &fresh), but("Unread"));
        assert_eq!(names(Some(ClaudeState::Waiting), true, &fresh), but("Needs you"), "a blocked one too");
        assert!(move_targets(Some(ClaudeState::Waiting), false, false, &fresh, NOW).is_empty(), "an ended shell is done");
        assert_eq!(names(Some(ClaudeState::Waiting), false, &fresh.moved_to(MoveTo::Later(NOW + 9))), but("Later"), "from Later, every section");
        // Every move lands where it says, from any state.
        for state in [None, Some(ClaudeState::Idle), Some(ClaudeState::Working), Some(ClaudeState::Waiting)] {
            for blocked in [false, true] {
                for (label, to) in move_targets(state, blocked, true, &fresh, NOW) {
                    let lands = of(state, blocked, true, &fresh.moved_to(to), NOW);
                    assert_eq!(lands.label(), label, "{label} from {state:?} (blocked {blocked})");
                }
            }
        }
        // A later time that has come brings a waiting session back as new.
        let m = fresh.moved_to(MoveTo::Later(NOW + 5));
        assert_eq!((of(Some(ClaudeState::Waiting), false, true, &m, NOW), of(Some(ClaudeState::Waiting), false, true, &m, NOW + 5)), (Attention::Later, Attention::Unread));
        assert_eq!(fresh.moved_to(MoveTo::To(Attention::Done)).since, fresh.since, "a move keeps the time of the session's last change");
        // A placement lasts until the agent next does something.
        let mut quiet = Marks { seen: true, ..fresh }.moved_to(MoveTo::To(Attention::Quiet));
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &quiet, NOW), Attention::Quiet);
        quiet.moved(NOW + 1);
        assert_eq!(of(Some(ClaudeState::Working), false, true, &quiet, NOW + 1), Attention::Working, "a prompt takes it back");
        let mut parked = fresh.moved_to(MoveTo::To(Attention::Unanswered));
        assert_eq!(of(None, false, true, &parked, NOW), Attention::Unanswered);
        parked.turned(false, NOW + 2);
        assert_eq!(of(Some(ClaudeState::Waiting), false, true, &parked, NOW + 2), Attention::Unread, "a new answer is new");
    }

    #[test]
    fn a_session_the_user_starts_is_new_until_something_happens_or_it_is_left_alone_for_a_while() {
        let mut m = Marks::default();
        m.started(NOW);
        assert_eq!(of(None, false, true, &m, NOW), Attention::New);
        assert_eq!(of(Some(ClaudeState::Idle), false, true, &m, NOW + NEW_FOR - 1), Attention::New, "a Claude Code started in it, not yet asked anything");
        assert_eq!(of(None, false, true, &m, NOW + NEW_FOR), Attention::Quiet, "forgotten: quiet");
        // Something happening in it ends it being new, and it does not come back.
        assert_eq!(of(Some(ClaudeState::Working), false, true, &m, NOW), Attention::Working);
        let mut worked = m;
        worked.moved(NOW + 5);
        assert_eq!(of(Some(ClaudeState::Idle), false, true, &worked, NOW + 6), Attention::Quiet);
        let mut answered = m;
        answered.turned(false, NOW + 5);
        assert_eq!(answered.new_until, None);
        // What the user says of it stands: unread, done, a block.
        assert_eq!(of(None, false, true, &m.moved_to(MoveTo::To(Attention::Done)), NOW), Attention::Done);
        assert_eq!(of(None, false, true, &m.moved_to(MoveTo::To(Attention::Unread)), NOW), Attention::Unread);
        assert_eq!(of(Some(ClaudeState::Waiting), true, true, &m, NOW), Attention::NeedsYou);
        assert_eq!(of(None, false, false, &m, NOW), Attention::Done, "its shell ended");
        // A session never started here (restored, or from before) is quiet from the first.
        assert_eq!(of(None, false, true, &Marks::default(), NOW), Attention::Quiet);
    }

    #[test]
    fn the_groups_are_listed_from_what_calls_for_the_user_to_what_is_put_away() {
        assert_eq!(Attention::ALL.map(Attention::label), ["Needs you", "Unread", "New", "Unanswered", "Working", "Quiet", "Later", "Done"]);
        assert_eq!(Attention::ALL.map(Attention::calls), [true, true, false, false, false, false, false, false]);
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
