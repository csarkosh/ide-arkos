//! What the bell shows: one notice per kind, newest first, read once the popover has been
//! opened. An update waiting keeps its dot whatever was read. A notice that goes (its trouble
//! put right, or "Updated" once seen) stays below as read history, the few most recent (Cyrus,
//! 2026-11-16: "I'd still like to see past notifications"). Pure bookkeeping; the window feeds
//! it and draws it.

use std::time::{Duration, SystemTime};

/// What a notice is about; a kind holds at most one notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UpdateReady,
    Updated,
    Daemon,
    ConfigError,
    ThemeError,
    ThemeRetired,
    /// Arkos switched Claude Code to its Arkos theme; running sessions need a restart.
    ClaudeColours,
    /// Arkos set up Claude Code's hooks; a Claude Code already running needs a restart.
    ClaudeHooks,
}

/// What clicking a notice does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    UpdateNow,
    OpenConfig,
    OpenTheme,
}

/// One line in the bell's list: its kind, the text shown, when it last changed, and what a click does.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub kind: Kind,
    pub text: String,
    pub at: SystemTime,
    pub action: Option<Action>,
}

/// How many past notices the bell keeps.
pub const PAST_KEPT: usize = 5;

/// The notices now standing, at most one per kind, and the few that went most recently.
#[derive(Default)]
pub struct Notices {
    items: Vec<Notice>,
    past: Vec<Notice>,
    opened_at: Option<SystemTime>,
    seen_updated: bool,
}

impl Notices {
    /// Replaces the kind's notice. The same text keeps its time, so a condition that is
    /// reported again every tick does not read as new news.
    pub fn set(&mut self, kind: Kind, text: impl Into<String>, action: Option<Action>, now: SystemTime) {
        let text = text.into();
        match self.items.iter_mut().find(|n| n.kind == kind) {
            Some(n) if n.text == text => n.action = action,
            Some(n) => {
                n.text = text;
                n.at = now;
                n.action = action;
            }
            None => {
                crate::telemetry::notice(kind);
                self.items.push(Notice { kind, text, at: now, action });
            }
        }
        self.items.sort_by(|a, b| b.at.cmp(&a.at));
    }

    /// Drops the kind's notice into the history; false if there was none.
    pub fn clear(&mut self, kind: Kind) -> bool {
        let Some(i) = self.items.iter().position(|n| n.kind == kind) else { return false };
        let gone = self.items.remove(i);
        self.keep(gone);
        true
    }

    /// A notice that went, kept as history: newest first, with no button (what it acted on has
    /// gone), the same text replacing its older line, at most `PAST_KEPT`. The connection and a
    /// waiting update are status while they hold, never history.
    fn keep(&mut self, gone: Notice) {
        if matches!(gone.kind, Kind::Daemon | Kind::UpdateReady) {
            return;
        }
        self.past.retain(|p| !(p.kind == gone.kind && p.text == gone.text));
        self.past.insert(0, Notice { action: None, ..gone });
        self.past.sort_by(|a, b| b.at.cmp(&a.at));
        self.past.truncate(PAST_KEPT);
    }

    /// The notices that went, newest first: drawn as read.
    pub fn past(&self) -> &[Notice] {
        &self.past
    }

    /// The notices, newest first.
    pub fn items(&self) -> &[Notice] {
        &self.items
    }

    /// Whether `n` is later than `since`; with no `since` everything is.
    pub fn unread_since(&self, n: &Notice, since: Option<SystemTime>) -> bool {
        since.map_or(true, |o| n.at > o)
    }

    /// Whether `n` arrived after the popover was last opened or marked read.
    pub fn unread(&self, n: &Notice) -> bool {
        self.unread_since(n, self.opened_at)
    }

    /// How many notices are unread.
    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|n| self.unread(n)).count()
    }

    /// Whether the bell wears its dot: anything unread, or an update waiting.
    pub fn dot(&self) -> bool {
        self.unread_count() > 0 || self.has(Kind::UpdateReady)
    }

    /// When the popover was last opened or marked read, if ever.
    pub fn opened_at(&self) -> Option<SystemTime> {
        self.opened_at
    }

    /// The popover opened: everything is read now.
    pub fn opened(&mut self, now: SystemTime) {
        self.opened_at = Some(now);
        self.seen_updated = self.has(Kind::Updated);
    }

    /// Marks everything now standing as read without touching the Updated bookkeeping, for a notice
    /// that arrived while the popover was open.
    pub fn mark_read(&mut self, now: SystemTime) {
        self.opened_at = Some(now);
    }

    /// The popover closed: an Updated notice that was on show has been seen and goes into the
    /// history.
    pub fn closed(&mut self) {
        if self.seen_updated {
            self.clear(Kind::Updated);
        }
        self.seen_updated = false;
    }

    /// Whether a notice of this kind is standing.
    pub fn has(&self, kind: Kind) -> bool {
        self.items.iter().any(|n| n.kind == kind)
    }
}

/// How long ago `at` was, for the notice list. A clock that went back reads "just now".
pub fn relative_time(at: SystemTime, now: SystemTime) -> String {
    let Ok(ago) = now.duration_since(at) else {
        return "just now".into();
    };
    let secs = ago.as_secs();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86_400 {
        format!("{} h ago", secs / 3600)
    } else if secs < 2 * 86_400 {
        let (h, m) = local_clock(at);
        format!("yesterday {h:02}:{m:02}")
    } else {
        format!("{} days ago", secs / 86_400)
    }
}

/// The local hour and minute of `at`.
fn local_clock(at: SystemTime) -> (i32, i32) {
    let secs = at.duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as libc::time_t;
    // SAFETY: localtime_r writes only into the tm we hand it, and reads the time_t by pointer.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    (tm.tm_hour, tm.tm_min)
}

/// The notice text for the update-ready field the bar used to show: its "update ready: "
/// prefix capitalised; a text without the prefix is kept as it is.
pub fn update_ready_text(bar_text: &str) -> String {
    match crate::update::ready_version(bar_text) {
        Some(v) => format!("Update ready: {v}"),
        None => bar_text.to_string(),
    }
}

/// What the Daemon kind shows: connection trouble first, then a failed update, then an
/// unreachable update server, else nothing.
pub fn daemon_notice(label: &str, update_error: Option<&str>, unreachable: Option<&str>) -> Option<String> {
    if label != "arkosd connected" && label != "arkosd connecting" {
        Some(label.to_string())
    } else if let Some(e) = update_error {
        Some(if e.starts_with("update") { e.into() } else { format!("update failed: {e}") })
    } else {
        unreachable.map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    #[test]
    fn notices_are_newest_first_and_one_per_kind() {
        let mut n = Notices::default();
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(10));
        n.set(Kind::UpdateReady, "Update ready: 0.2.15", Some(Action::UpdateNow), t(20));
        assert_eq!(n.items().iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["Update ready: 0.2.15", "arkosd reconnecting"]);
        assert_eq!(n.items()[0].action, Some(Action::UpdateNow));
    }

    #[test]
    fn setting_a_kind_again_replaces_it() {
        let mut n = Notices::default();
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(10));
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(50));
        assert_eq!(n.items().len(), 1);
        assert_eq!(n.items()[0].at, t(10), "the same text keeps its time");
        n.set(Kind::Daemon, "arkosd: see banner", None, t(60));
        assert_eq!((n.items().len(), n.items()[0].at), (1, t(60)), "a new text is new news");
        assert!(n.clear(Kind::Daemon));
        assert!(!n.clear(Kind::Daemon));
        assert!(n.items().is_empty());
    }

    #[test]
    fn unread_follows_the_last_opening() {
        let mut n = Notices::default();
        n.set(Kind::ConfigError, "config.toml line 3: bad key", Some(Action::OpenConfig), t(10));
        assert!(n.dot());
        assert_eq!(n.unread_count(), 1);
        n.opened(t(20));
        assert_eq!(n.opened_at(), Some(t(20)));
        assert!(!n.dot());
        assert_eq!(n.unread_count(), 0);
        n.set(Kind::ThemeError, "theme x: missing color", Some(Action::OpenTheme), t(30));
        assert!(n.dot(), "news after the last opening is unread");
        assert!(n.unread(&n.items()[0]));
        assert!(!n.unread(&n.items()[1]));
        assert!(n.unread_since(&n.items()[1], None), "with no opening on record everything is unread");
        assert!(!n.unread_since(&n.items()[1], Some(t(20))));
    }

    #[test]
    fn an_update_ready_keeps_the_dot_until_cleared() {
        let mut n = Notices::default();
        n.set(Kind::UpdateReady, "Update ready: 0.2.15", Some(Action::UpdateNow), t(10));
        n.opened(t(20));
        assert!(n.dot(), "an update waiting is always news");
        assert_eq!(n.unread_count(), 0, "but it is not counted as unread");
        n.clear(Kind::UpdateReady);
        assert!(!n.dot());
    }

    #[test]
    fn marking_read_covers_a_notice_that_came_while_open() {
        let mut n = Notices::default();
        n.set(Kind::Updated, "updated", None, t(10));
        n.opened(t(20));
        n.set(Kind::Daemon, "arkosd gone", None, t(30));
        assert_eq!(n.unread_count(), 1);
        n.mark_read(t(40));
        assert_eq!(n.unread_count(), 0);
        assert!(n.has(Kind::Updated), "mark_read leaves the Updated bookkeeping alone");
        n.closed();
        assert!(!n.has(Kind::Updated), "the Updated notice seen at opening still goes");
    }

    #[test]
    fn the_updated_notice_goes_once_seen() {
        let mut n = Notices::default();
        n.set(Kind::Updated, "Updated to 0.2.14", None, t(0));
        n.closed();
        assert!(n.has(Kind::Updated), "closing without opening is not seeing it");
        n.opened(t(5));
        n.closed();
        assert!(!n.has(Kind::Updated));
    }

    #[test]
    fn a_notice_that_goes_is_kept_as_read_history_newest_first() {
        let mut n = Notices::default();
        n.set(Kind::ConfigError, "config.toml line 3: bad key", Some(Action::OpenConfig), t(10));
        n.set(Kind::ClaudeHooks, "Arkos set up Claude Code's hooks", None, t(20));
        n.clear(Kind::ConfigError);
        n.set(Kind::Updated, "Updated to 0.8.41", None, t(30));
        n.opened(t(40));
        n.closed();
        let past: Vec<&str> = n.past().iter().map(|x| x.text.as_str()).collect();
        assert_eq!(past, ["Updated to 0.8.41", "config.toml line 3: bad key"], "the Updated notice seen, then the error put right");
        assert_eq!(n.past()[1].action, None, "a past notice's button would act on something gone");
        assert_eq!(n.past()[1].at, t(10), "it keeps the time it came");
        assert_eq!(n.items().len(), 1, "the hooks notice still stands");
        assert!(!n.dot(), "history is never news");
    }

    #[test]
    fn the_history_keeps_a_few_and_one_of_each_text() {
        let mut n = Notices::default();
        for i in 0..(PAST_KEPT as u64 + 3) {
            n.set(Kind::ThemeError, format!("theme {i}: bad colour"), None, t(i));
            n.clear(Kind::ThemeError);
        }
        assert_eq!(n.past().len(), PAST_KEPT);
        assert_eq!(n.past()[0].text, format!("theme {}: bad colour", PAST_KEPT + 2), "the newest kept");
        n.set(Kind::ThemeError, "theme 0: bad colour", None, t(100));
        n.clear(Kind::ThemeError);
        n.set(Kind::ThemeError, "theme 0: bad colour", None, t(200));
        n.clear(Kind::ThemeError);
        assert_eq!(n.past().iter().filter(|x| x.text == "theme 0: bad colour").count(), 1, "the same news again replaces its old line");
        assert_eq!(n.past()[0].at, t(200));
    }

    #[test]
    fn live_status_leaves_no_history() {
        // The connection and a waiting update are status while they hold: a reconnect during
        // every update, or an update applied, is not news worth keeping.
        let mut n = Notices::default();
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(10));
        n.set(Kind::UpdateReady, "Update ready: 0.8.41", Some(Action::UpdateNow), t(20));
        n.clear(Kind::Daemon);
        n.clear(Kind::UpdateReady);
        assert!(n.past().is_empty());
    }

    #[test]
    fn the_daemon_notice_ranks_connection_over_update_trouble() {
        assert_eq!(daemon_notice("arkosd reconnecting", Some("update failed: x"), Some("updates unreachable for 2 days")), Some("arkosd reconnecting".into()));
        assert_eq!(daemon_notice("arkosd: see banner", None, None), Some("arkosd: see banner".into()));
        assert_eq!(daemon_notice("connection refused", None, None), Some("connection refused".into()), "a status text with no known prefix is passed through");
        assert_eq!(daemon_notice("arkosd connected", Some("bad signature"), Some("updates unreachable for 2 days")), Some("update failed: bad signature".into()));
        assert_eq!(daemon_notice("arkosd connected", Some("update failed: bad signature"), None), Some("update failed: bad signature".into()), "never doubled");
        assert_eq!(daemon_notice("arkosd connected", None, Some("updates unreachable for 2 days")), Some("updates unreachable for 2 days".into()));
        assert_eq!(daemon_notice("arkosd connected", None, None), None);
        assert_eq!(daemon_notice("arkosd connecting", None, None), None, "the moment at every launch is not news");
        assert_eq!(daemon_notice("arkosd connecting", Some("x"), None), Some("update failed: x".into()));
    }

    #[test]
    fn the_update_badge_text_becomes_the_notice_text() {
        assert_eq!(update_ready_text("update ready: 0.2.15"), "Update ready: 0.2.15");
        assert_eq!(update_ready_text("something else"), "something else");
    }

    #[test]
    fn relative_times_read_naturally() {
        let now = t(100_000);
        assert_eq!(relative_time(now - Duration::from_secs(5), now), "just now");
        assert_eq!(relative_time(now - Duration::from_secs(59), now), "just now");
        assert_eq!(relative_time(now - Duration::from_secs(60), now), "1 min ago");
        assert_eq!(relative_time(now - Duration::from_secs(59 * 60), now), "59 min ago");
        assert_eq!(relative_time(now - Duration::from_secs(3600), now), "1 h ago");
        assert_eq!(relative_time(now - Duration::from_secs(23 * 3600), now), "23 h ago");
        assert_eq!(relative_time(now - Duration::from_secs(3 * 86_400), now), "3 days ago");
        assert_eq!(relative_time(now + Duration::from_secs(5), now), "just now", "a clock that went back");
        let yesterday = relative_time(now - Duration::from_secs(30 * 3600), now);
        assert!(yesterday.starts_with("yesterday ") && yesterday.len() == 15 && yesterday.as_bytes()[12] == b':', "{yesterday}");
    }
}
