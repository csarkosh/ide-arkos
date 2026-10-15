//! What "Check for updates" says, decided from the daemon's status answers, and which update
//! text the title bar shows. Pure, so it is tested.
use protocol::StagedUpdate;

/// One status answer, in either protocol shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub staged: Option<StagedUpdate>,
    pub last_error: Option<String>,
    pub unreachable_for_secs: Option<u64>,
    /// None for a protocol 4 daemon, which has no check state to report.
    pub checking: Option<bool>,
    pub last_check_unix: Option<u64>,
    pub transient_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ready(String),
    Failed(String),
    Unreachable(String),
    StillRunning,
    NotReachedYet,
    NoUpdate,
}

/// The poll after a check the user asked for: `asked_at` is when the daemon accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poll {
    pub asked_at: u64,
    pub polls: u32,
}

pub const POLL_EVERY_MS: u64 = 500;
pub const MAX_POLLS: u32 = 60;

impl Poll {
    pub fn new(asked_at: u64) -> Self {
        Poll { asked_at, polls: 0 }
    }

    /// None: ask again after `POLL_EVERY_MS`. Some: the notice to show.
    pub fn answer(&mut self, a: &Answer) -> Option<Verdict> {
        self.polls += 1;
        match a.checking {
            // Protocol 4: one answer, read as today, with the never-reached case named.
            None => Some(Self::verdict(a, true)),
            Some(checking) => {
                let done = !checking && a.last_check_unix.is_some_and(|t| t >= self.asked_at);
                if done {
                    Some(Self::verdict(a, false))
                } else if self.polls >= MAX_POLLS {
                    Some(Verdict::StillRunning)
                } else {
                    None
                }
            }
        }
    }

    fn verdict(a: &Answer, old_daemon: bool) -> Verdict {
        if let Some(s) = &a.staged {
            return Verdict::Ready(s.version.clone());
        }
        if let Some(e) = &a.last_error {
            return Verdict::Failed(e.clone());
        }
        if let Some(e) = &a.transient_error {
            return Verdict::Unreachable(e.clone());
        }
        if old_daemon && a.unreachable_for_secs.is_none() {
            return Verdict::NotReachedYet;
        }
        if a.unreachable_for_secs.is_some_and(|s| s > 0) {
            return Verdict::Unreachable("the last check did not reach the update server".into());
        }
        Verdict::NoUpdate
    }
}

/// The badge's ready text: the staged release, else what the launch path holds.
pub fn ready_text(staged: Option<&StagedUpdate>, on_disk: Option<&str>) -> Option<String> {
    staged.map(crate::update::status_text_for).or_else(|| on_disk.map(str::to_owned))
}

/// The title bar's update item: what can be acted on first.
pub fn title_text<'a>(ready: Option<&'a str>, error: Option<&'a str>, unreachable: Option<&'a str>) -> Option<&'a str> {
    ready.or(error.map(|_| "update failed")).or(unreachable)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn answer(checking: Option<bool>, last: Option<u64>) -> Answer {
        Answer { staged: None, last_error: None, unreachable_for_secs: Some(0), checking, last_check_unix: last, transient_error: None }
    }
    #[test]
    fn the_poll_waits_for_a_check_that_finished_after_the_ask() {
        let mut p = Poll::new(100);
        assert_eq!(p.answer(&answer(Some(false), Some(90))), None, "an older check");
        assert_eq!(p.answer(&answer(Some(true), Some(90))), None, "running");
        assert_eq!(p.answer(&answer(Some(false), Some(100))), Some(Verdict::NoUpdate));
    }
    #[test]
    fn each_outcome_has_its_notice() {
        let mut p = Poll::new(1);
        let mut a = answer(Some(false), Some(5));
        a.staged = Some(StagedUpdate { version: "1.2.3".into(), git: "g".into() });
        assert_eq!(p.answer(&a), Some(Verdict::Ready("1.2.3".into())));
        let mut a = answer(Some(false), Some(5));
        a.last_error = Some("bad hash".into());
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::Failed("bad hash".into())));
        let mut a = answer(Some(false), Some(5));
        a.transient_error = Some("offline".into());
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::Unreachable("offline".into())));
    }
    #[test]
    fn a_check_that_outlives_the_connection_times_out_quietly() {
        let mut p = Poll::new(1);
        for _ in 0..MAX_POLLS - 1 {
            assert_eq!(p.answer(&answer(Some(true), None)), None);
        }
        assert_eq!(p.answer(&answer(Some(true), None)), Some(Verdict::StillRunning));
    }
    #[test]
    fn a_protocol_4_daemon_answers_once_and_names_the_never_reached_case() {
        let mut a = answer(None, None);
        a.unreachable_for_secs = None;
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::NotReachedYet));
        assert_eq!(Poll::new(1).answer(&answer(None, None)), Some(Verdict::NoUpdate));
    }
    #[test]
    fn the_title_bar_ranks_ready_then_failed_then_unreachable() {
        assert_eq!(title_text(Some("update ready: 1"), Some("e"), Some("updates unreachable for 2 days")), Some("update ready: 1"));
        assert_eq!(title_text(None, Some("e"), Some("u")), Some("update failed"));
        assert_eq!(title_text(None, None, Some("u")), Some("u"));
        assert_eq!(title_text(None, None, None), None);
    }
    #[test]
    fn the_ready_text_survives_a_failure_and_outranks_it_in_the_title_bar() {
        let staged = StagedUpdate { version: "2.0".into(), git: "g".into() };
        // `note_update_status(None, Some("bad hash"), ..)` drops `staged`; the launch path's text stays.
        let ready = ready_text(None, Some("update ready: 2.0"));
        assert_eq!(ready.as_deref(), Some("update ready: 2.0"));
        assert_eq!(title_text(ready.as_deref(), Some("bad hash"), None), Some("update ready: 2.0"));
        assert_eq!(ready_text(Some(&staged), None).as_deref(), Some("update ready: 2.0"));
        assert_eq!(ready_text(None, None), None);
    }
}
