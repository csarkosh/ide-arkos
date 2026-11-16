//! A session's reminder (sessions hold grids spec, section 4): a time, and a repeat. Pending, the
//! session is listed under Scheduled; when its time comes it comes up, labelled, until the user
//! looks, and a repeating one then waits for its next time. Pure, so it is tested; the window
//! keeps one per page and saves them with its workspace.
use crate::attention::Attention;
use serde::{Deserialize, Serialize};

/// How a reminder repeats, counted from the time first chosen (`Reminder::anchor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Never,
    /// Every day at that time.
    Daily,
    /// Monday to Friday at that time.
    Weekdays,
    /// On that weekday each week.
    Weekly,
    /// On that day of each month, or the month's last day when it is shorter.
    Monthly,
}

/// Why a reminder is up out of Scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Up {
    /// Its time came: the row carries the Reminder label.
    Time,
    /// A terminal of the session began a new turn first: listed as that turn says.
    Turn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    /// The time it comes up next, in milliseconds since the Unix epoch.
    pub at: u64,
    #[serde(default)]
    pub repeat: Repeat,
    /// The time first chosen: a repeat keeps its hour, minute, weekday and day of the month.
    pub anchor: u64,
    /// Up out of Scheduled, and why; None while it waits for `at`.
    #[serde(default)]
    pub up: Option<Up>,
}

/// Where a reminder stands at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Pending,
    /// Its time came and the user has not looked.
    Came,
    /// A new turn brought the session up before its time.
    Turned,
}

impl Reminder {
    pub fn new(at: u64, repeat: Repeat) -> Reminder {
        Reminder { at, repeat, anchor: at, up: None }
    }

    pub fn state(&self, now: u64) -> State {
        match self.up {
            Some(Up::Turn) => State::Turned,
            Some(Up::Time) => State::Came,
            None if self.at <= now => State::Came,
            None => State::Pending,
        }
    }

    /// A terminal of the session began a new turn (an agent finished or asks the user): a
    /// pending one-time reminder is dropped, as a put-off session always came back with its
    /// news; a repeating one stands aside until the user looks. None: drop it.
    pub fn turned(self, now: u64) -> Option<Reminder> {
        match (self.state(now), self.repeat) {
            (State::Pending, Repeat::Never) => None,
            (State::Pending, _) => Some(Reminder { up: Some(Up::Turn), ..self }),
            _ => Some(self),
        }
    }

    /// The user looked at the session at `now`: a reminder that is up is done, or with a repeat
    /// waits for its next time. None: drop it.
    pub fn looked(self, now: u64) -> Option<Reminder> {
        if self.state(now) == State::Pending {
            return Some(self);
        }
        match self.repeat {
            Repeat::Never => None,
            repeat => Some(Reminder { at: next_after(self.anchor, repeat, now), up: None, ..self }),
        }
    }

    /// Where a session with this reminder is listed, from `base`, where its terminals put it,
    /// and whether its row carries the Reminder label.
    pub fn listed(&self, base: Attention, now: u64) -> (Attention, bool) {
        match self.state(now) {
            State::Pending => (Attention::Scheduled, false),
            State::Turned => (base, false),
            State::Came => match base {
                Attention::NeedsYou | Attention::Working => (base, true),
                _ => (Attention::Unread, true),
            },
        }
    }
}

/// A moment's local date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    /// 0 Sunday to 6 Saturday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
}

/// `ms` (milliseconds since the Unix epoch) in local time.
pub fn local(ms: u64) -> Local {
    let secs = (ms / 1000) as libc::time_t;
    // SAFETY: `localtime_r` writes only the `tm` passed, which lives for the call; a zeroed
    // `tm` is a valid value for it to fill.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    Local { year: tm.tm_year + 1900, month: (tm.tm_mon + 1) as u32, day: tm.tm_mday as u32, weekday: tm.tm_wday as u32, hour: tm.tm_hour as u32, minute: tm.tm_min as u32 }
}

/// The moment of a local date and time, in milliseconds since the Unix epoch. A day past the
/// month's end runs into the next month, as `mktime` does; daylight time is decided for that day.
pub fn at_local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> u64 {
    // SAFETY: `mktime` reads and writes only the `tm` passed, which lives for the call.
    let secs = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = year - 1900;
        tm.tm_mon = month as i32 - 1;
        tm.tm_mday = day as i32;
        tm.tm_hour = hour as i32;
        tm.tm_min = minute as i32;
        tm.tm_isdst = -1;
        libc::mktime(&mut tm)
    };
    secs.max(0) as u64 * 1000
}

/// The days in a month of a year.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// The first time after `now` that a reminder first set for `anchor` comes up with `repeat`.
/// `Never` has no next time: `anchor` itself.
pub fn next_after(anchor: u64, repeat: Repeat, now: u64) -> u64 {
    let a = local(anchor);
    let n = local(now.max(anchor));
    let on = |year: i32, month: u32, day: u32| at_local(year, month, day, a.hour, a.minute);
    match repeat {
        Repeat::Never => anchor,
        Repeat::Daily | Repeat::Weekdays | Repeat::Weekly => (0..=8u32)
            .map(|d| on(n.year, n.month, n.day + d))
            .find(|t| {
                let w = local(*t).weekday;
                *t > now
                    && match repeat {
                        Repeat::Weekdays => (1..=5).contains(&w),
                        Repeat::Weekly => w == a.weekday,
                        _ => true,
                    }
            })
            .unwrap_or(now + 86_400_000),
        Repeat::Monthly => (0..=13u32)
            .map(|m| {
                let months = n.month - 1 + m;
                let (year, month) = (n.year + (months / 12) as i32, months % 12 + 1);
                on(year, month, a.day.min(days_in_month(year, month)))
            })
            .find(|t| *t > now)
            .unwrap_or(now + 31 * 86_400_000),
    }
}

/// A repeat as the picker and a row name it, for a reminder first set for `anchor`.
pub fn repeat_label(repeat: Repeat, anchor: u64) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let a = local(anchor);
    match repeat {
        Repeat::Never => "Never".into(),
        Repeat::Daily => "Every day".into(),
        Repeat::Weekdays => "Every weekday".into(),
        Repeat::Weekly => format!("Every week on {}", DAYS[a.weekday as usize]),
        Repeat::Monthly => format!("Every month on the {}", ordinal(a.day)),
    }
}

/// 1st, 2nd, 3rd, 4th, 11th, 21st.
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;
    const DAY: u64 = 24 * HOUR;

    fn nine_on(year: i32, month: u32, day: u32) -> u64 {
        at_local(year, month, day, 9, 0)
    }

    #[test]
    fn a_reminder_is_pending_until_its_time_then_up_with_its_label() {
        let at = nine_on(2026, 11, 17);
        let r = Reminder::new(at, Repeat::Never);
        assert_eq!(r.listed(Attention::Quiet, at - 1), (Attention::Scheduled, false));
        assert_eq!(r.listed(Attention::Working, at - 1), (Attention::Scheduled, false), "pending beats what its terminals do");
        assert_eq!(r.listed(Attention::Quiet, at), (Attention::Unread, true));
        assert_eq!(r.listed(Attention::Unanswered, at), (Attention::Unread, true));
        assert_eq!(r.listed(Attention::Working, at), (Attention::Working, true), "a session at work stays at work");
        assert_eq!(r.listed(Attention::NeedsYou, at), (Attention::NeedsYou, true));
    }

    #[test]
    fn a_look_ends_a_one_time_reminder_and_sends_a_repeating_one_to_its_next_time() {
        let at = nine_on(2026, 11, 17);
        assert_eq!(Reminder::new(at, Repeat::Never).looked(at + HOUR), None);
        let r = Reminder::new(at, Repeat::Daily).looked(at + HOUR).unwrap();
        assert_eq!(r.at, nine_on(2026, 11, 18));
        assert_eq!(r.listed(Attention::Quiet, at + HOUR), (Attention::Scheduled, false), "back under Scheduled");
        let pending = Reminder::new(at, Repeat::Never);
        assert_eq!(pending.looked(at - HOUR), Some(pending), "a look before its time changes nothing");
    }

    #[test]
    fn a_new_turn_drops_a_pending_one_time_reminder_and_lifts_a_repeating_one_unlabelled() {
        let at = nine_on(2026, 11, 17);
        assert_eq!(Reminder::new(at, Repeat::Never).turned(at - HOUR), None);
        let r = Reminder::new(at, Repeat::Weekdays).turned(at - HOUR).unwrap();
        assert_eq!(r.listed(Attention::Unread, at - HOUR), (Attention::Unread, false));
        assert_eq!(r.listed(Attention::Working, at - HOUR), (Attention::Working, false));
        let back = r.looked(at - HOUR / 2).unwrap();
        assert_eq!(back.state(at - HOUR / 2), State::Pending, "a look sends it back to wait");
        assert_eq!(back.at, at, "for the time still to come");
        let came = Reminder::new(at, Repeat::Never);
        assert_eq!(came.turned(at + 1), Some(came), "one that came stays up until a look");
    }

    #[test]
    fn every_repeat_keeps_its_time_of_day_and_its_day_through_a_year() {
        let anchor = at_local(2026, 1, 31, 9, 30);
        let a = local(anchor);
        let mut now = anchor;
        for _ in 0..400 {
            now += DAY - HOUR / 3;
            for repeat in [Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Monthly] {
                let next = next_after(anchor, repeat, now);
                let l = local(next);
                assert!(next > now, "{repeat:?} after {now}");
                assert_eq!((l.hour, l.minute), (9, 30), "{repeat:?}: the time of day, daylight time or not");
                match repeat {
                    Repeat::Daily => assert!(next - now <= DAY + HOUR, "{repeat:?}: within a day"),
                    Repeat::Weekdays => {
                        assert!((1..=5).contains(&l.weekday));
                        assert!(next - now <= 3 * DAY + HOUR);
                    }
                    Repeat::Weekly => {
                        assert_eq!(l.weekday, a.weekday);
                        assert!(next - now <= 7 * DAY + HOUR);
                    }
                    Repeat::Monthly => assert_eq!(l.day, 31.min(days_in_month(l.year, l.month)), "the 31st, or the month's last day"),
                    Repeat::Never => {}
                }
            }
        }
    }

    #[test]
    fn a_monthly_reminder_on_the_31st_comes_on_the_last_day_of_a_short_month() {
        let anchor = at_local(2026, 1, 31, 9, 0);
        assert_eq!(next_after(anchor, Repeat::Monthly, anchor), at_local(2026, 2, 28, 9, 0));
        assert_eq!(next_after(anchor, Repeat::Monthly, at_local(2026, 2, 28, 10, 0)), at_local(2026, 3, 31, 9, 0));
        assert_eq!(next_after(anchor, Repeat::Monthly, at_local(2027, 12, 31, 10, 0)), at_local(2028, 1, 31, 9, 0), "across a year's end");
        assert_eq!(days_in_month(2028, 2), 29);
    }

    #[test]
    fn a_weekday_reminder_set_on_friday_comes_on_monday() {
        // 2026-11-20 is a Friday.
        let friday = nine_on(2026, 11, 20);
        assert_eq!(local(friday).weekday, 5);
        assert_eq!(next_after(friday, Repeat::Weekdays, friday), nine_on(2026, 11, 23));
        assert_eq!(next_after(friday, Repeat::Weekly, friday), nine_on(2026, 11, 27));
    }

    #[test]
    fn repeats_are_named_for_the_day_chosen() {
        let friday = nine_on(2026, 11, 20);
        assert_eq!(repeat_label(Repeat::Weekly, friday), "Every week on Friday");
        assert_eq!(repeat_label(Repeat::Monthly, friday), "Every month on the 20th");
        assert_eq!(repeat_label(Repeat::Weekdays, friday), "Every weekday");
        assert_eq!((ordinal(1), ordinal(2), ordinal(3), ordinal(11), ordinal(12), ordinal(22), ordinal(31)), ("1st".into(), "2nd".into(), "3rd".into(), "11th".into(), "12th".into(), "22nd".into(), "31st".into()));
    }

    #[test]
    fn a_reminder_reads_back_as_it_was_saved() {
        let r = Reminder { at: 5, repeat: Repeat::Monthly, anchor: 3, up: Some(Up::Turn) };
        let back: Reminder = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
