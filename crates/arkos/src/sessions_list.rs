//! The window's sessions as a list sorted by what they ask of the user (layouts spec, "The
//! session list"): the Inbox's sidebar (the Grid's has none since 2026-11-14). The
//! grouping, the menu's items and the times are pure and tested; the rows read the root.
use crate::attention::{self, Attention};
use crate::effects::blocks;
use crate::motion;
use crate::pane_view::TabMark;
use crate::sessions::now_ms;
use crate::reminder::{self, Reminder};
use crate::tree::{PageKey, PaneId, Tab, TabId};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful};
use protocol::SessionId;
use std::time::Instant;

/// `text` drawn heavier than its font's own weight: drawn twice, the second half a point to the
/// right, so it thickens by one device pixel on a Retina screen in any face, the pixel faces
/// with a single weight included.
fn heavy(text: &'static str, color: gpui::Hsla) -> Div {
    div().relative().text_color(color).child(text).child(div().absolute().top_0().left(px(0.5)).child(text))
}

/// The keys a session row's highlight mark is reported under, apart from every other row's.
const MARK_LIVE: u64 = 0x5E55_1000_0000_0000;
const MARK_ENDED: u64 = 0x5E55_2000_0000_0000;

/// A session's row: two lines, its name and age over its line.
pub const ROW_H: f32 = 40.0;
/// A group's heading.
pub const GROUP_H: f32 = 20.0;

/// How many closed sessions the list keeps for resuming; an older one is forgotten.
pub const MAX_ENDED: usize = 50;

/// A Claude Code session whose tab was closed, kept so it can be resumed: what the list showed
/// of it, and the command that resumes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ended {
    /// The daemon's session it ran in: within the closed tab's minute that shell comes back
    /// itself instead of a resume in a new one.
    pub session: u64,
    pub name: String,
    pub line: String,
    /// The folder its shell ran in, when that was not the project.
    #[serde(default)]
    pub cwd: Option<std::path::PathBuf>,
    /// The command that resumes it, as argv.
    pub resume: Vec<String>,
    /// When its tab closed, in milliseconds since the Unix epoch.
    pub ended: u64,
}

/// `list` with `new` at its head: an earlier entry for the same conversation (the same resume
/// command) is replaced, and the list is cut to `MAX_ENDED`.
pub fn keep_ended(list: &mut Vec<Ended>, new: Ended) {
    list.retain(|e| e.resume != new.resume);
    list.insert(0, new);
    list.truncate(MAX_ENDED);
}

/// One command line for a shell that types `argv`: every word single-quoted, then Enter.
pub fn resume_line(argv: &[String]) -> Vec<u8> {
    let mut line = argv.iter().map(|a| format!("'{}'", a.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
    line.push('\r');
    line.into_bytes()
}

/// Which row a menu or a click is for: a session of the window, by its page, or a closed one
/// kept for resuming, by its place in the kept list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKey {
    Live(PageKey),
    Ended(usize),
}

/// One session of the window as the list shows it: a page and its terminals (sessions hold
/// grids spec, section 2), or a closed Claude Code session kept for resuming.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// A closed session kept for resuming, by its place in the kept list; None for a session
    /// the window shows.
    pub ended: Option<usize>,
    /// The page; 0 for a closed session.
    pub page: PageKey,
    /// The lead terminal's session: the one that decides where the row is listed.
    pub session: SessionId,
    pub pane: PaneId,
    pub idx: usize,
    /// The original terminal's name: the user's own for it, else its program's title.
    pub name: String,
    /// What the lead's agent asks or last said; for a session with nothing said, its folder.
    pub line: String,
    /// Every terminal's name, for the filter.
    pub names: Vec<String>,
    /// How many terminals the page holds.
    pub count: usize,
    pub attention: Attention,
    /// The group it is listed under: its attention, except for the session the user has just
    /// chosen, which stays among the unread until they choose another (`hold`).
    pub group: Attention,
    pub alive: bool,
    /// When its state last changed, in milliseconds since the Unix epoch; 0, unknown.
    pub since: u64,
    /// While its reminder is pending, the time it comes up.
    pub scheduled_until: Option<u64>,
    /// Its reminder repeats.
    pub repeats: bool,
    /// Its reminder's time came and the user has not looked: the row is labelled.
    pub reminded: bool,
}

/// One terminal of a page, as `page_entry` reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub session: SessionId,
    pub tab: TabId,
    pub pane: PaneId,
    pub idx: usize,
    pub name: String,
    pub line: String,
    pub attention: Attention,
    pub alive: bool,
    pub since: u64,
}

/// A page's row from its terminals (sessions hold grids spec, section 2): listed by the most
/// urgent of them (the lead; among equals the one `focused` in the page, else the oldest),
/// named for the oldest (the original), with the lead's line, under its reminder when it has
/// one. None for a page with no terminal.
pub fn page_entry(page: PageKey, mut terms: Vec<Term>, focused: Option<SessionId>, reminder: Option<&Reminder>, now: u64) -> Option<Entry> {
    terms.sort_by_key(|t| t.tab);
    let original = terms.first()?.clone();
    let lead = terms.iter().min_by_key(|t| (t.attention, Some(t.session) != focused, t.tab))?.clone();
    let (attention, reminded) = match reminder {
        Some(r) => r.listed(lead.attention, now),
        None => (lead.attention, false),
    };
    let pending = reminder.filter(|r| r.state(now) == reminder::State::Pending);
    Some(Entry {
        ended: None,
        page,
        session: lead.session,
        pane: lead.pane,
        idx: lead.idx,
        name: original.name,
        line: lead.line,
        names: terms.iter().map(|t| t.name.clone()).collect(),
        count: terms.len(),
        attention,
        group: attention,
        alive: terms.iter().any(|t| t.alive),
        since: terms.iter().map(|t| t.since).max().unwrap_or(0),
        scheduled_until: pending.map(|r| r.at),
        repeats: reminder.is_some_and(|r| r.repeat != reminder::Repeat::Never),
        reminded,
    })
}

/// The label a row carries beside its name, for its reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    /// Its time came and the user has not looked.
    Reminder,
    /// It has a reminder still to come but is listed elsewhere for now (at work, or with an
    /// answer the user has not read): it goes back under Scheduled once read.
    Scheduled,
}

/// A row's reminder label: Reminder while one is up; Scheduled while one is pending and the row
/// is listed under another group, so it reads as scheduled wherever it is; none under Scheduled
/// itself, whose heading says it.
pub fn badge(e: &Entry) -> Badge {
    if e.reminded {
        Badge::Reminder
    } else if e.scheduled_until.is_some() && e.group != Attention::Scheduled {
        Badge::Scheduled
    } else {
        Badge::None
    }
}

/// The entries under their groups, in `Attention::LISTED`'s order (New first), a group with nothing in it left
/// out. Within a group the newest change is first, and sessions of the same moment keep the
/// order they came in (the tabs' own), so the list does not shuffle on its own.
pub fn grouped(entries: Vec<Entry>) -> Vec<(Attention, Vec<Entry>)> {
    let mut groups: Vec<(Attention, Vec<Entry>)> = Vec::new();
    for kind in Attention::LISTED {
        let mut rows: Vec<Entry> = entries.iter().filter(|e| e.group == kind).cloned().collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| b.since.cmp(&a.since));
        groups.push((kind, rows));
    }
    groups
}

/// The entries with the session the user chose (`held`, and where it stood then) listed where
/// it stood: one that was unread stays under Unread, read, until another is chosen, so the row
/// clicked does not move from under the pointer; a reminder looked at that goes back to wait
/// for its next time included. One that has moved on (it works, it needs the user, it is done)
/// is listed where it now stands.
pub fn hold(mut entries: Vec<Entry>, held: Option<(PageKey, Attention)>) -> Vec<Entry> {
    if let Some((page, Attention::Unread)) = held {
        for e in entries.iter_mut().filter(|e| e.ended.is_none() && e.page == page && is_held(e.attention)) {
            e.group = Attention::Unread;
        }
    }
    entries
}

/// Whether a row held among the unread still stays there, standing where it now does.
pub fn is_held(now: Attention) -> bool {
    matches!(now, Attention::Unread | Attention::Unanswered | Attention::New | Attention::Quiet | Attention::Scheduled)
}

/// The name a session's row shows: while the user renames it, what they have typed so far (a
/// blank name gives the automatic one back, as it will when it is set); else the name they gave
/// it, else its program's title, else the shell's.
pub fn shown_name(saved: Option<&str>, typing: Option<&str>, title: Option<&str>, shell: &str) -> String {
    let given = match typing {
        Some(t) => Some(t.trim()).filter(|t| !t.is_empty()),
        None => saved,
    };
    given.or(title.filter(|t| !t.is_empty())).unwrap_or(shell).to_string()
}

/// What a terminal's title says that its name does not, for a row's second line: Claude Code
/// titles its terminal with the conversation's topic, behind a glyph that moves while it works.
/// None for no title, the name itself or the shell's.
pub fn title_line(title: &str, name: &str, shell: &str) -> Option<String> {
    let t = title.trim_start_matches(|c: char| !c.is_alphanumeric()).trim();
    (!t.is_empty() && t != name && t != shell).then(|| t.to_string())
}

/// The entries a filter leaves: those whose name or line holds every word typed, whatever its
/// case. A blank filter leaves them all.
pub fn matching(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            let text = format!("{} {} {}", e.name, e.line, e.names.join(" ")).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect()
}

/// The session to go to for "the next session that needs you": the first of Needs you, then of
/// Unread, then of Unanswered that is not `current`, and `current` itself when it is the only
/// one. None when no session is the user's move.
pub fn next_calling(groups: &[(Attention, Vec<Entry>)], current: Option<PageKey>) -> Option<PageKey> {
    let mine = |kind: &Attention| matches!(kind, Attention::NeedsYou | Attention::Unread | Attention::Unanswered);
    let all: Vec<PageKey> = groups.iter().filter(|(kind, _)| mine(kind)).flat_map(|(_, rows)| rows.iter().filter(|e| e.ended.is_none()).map(|e| e.page)).collect();
    all.iter().copied().find(|s| Some(*s) != current).or(all.first().copied())
}

/// How many sessions call for the user now: the count a Dock badge and the section's header show.
pub fn calling(groups: &[(Attention, Vec<Entry>)]) -> usize {
    groups.iter().filter(|(kind, _)| kind.calls()).map(|(_, rows)| rows.len()).sum()
}

/// The groups folded under their headings, as the user left them: `Attention::FOLDED_AT_FIRST`
/// until they fold or open one.
pub type Folded = std::collections::BTreeSet<Attention>;

/// Folds `kind`'s group, or opens it when folded.
pub fn toggle_fold(folded: &mut Folded, kind: Attention) {
    if !folded.remove(&kind) {
        folded.insert(kind);
    }
}

/// Whether `kind`'s rows are hidden under its heading: folded, and no filter typed, since a
/// filter shows every row it finds.
pub fn is_folded(folded: &Folded, kind: Attention, filtering: bool) -> bool {
    !filtering && folded.contains(&kind)
}

/// The key of `e`'s row: its page, or its place in the kept list for a closed session.
pub fn row_key(e: &Entry) -> RowKey {
    match e.ended {
        Some(i) => RowKey::Ended(i),
        None => RowKey::Live(e.page),
    }
}

/// The rows the list shows, in order: every group's rows but a folded group's (Chrome keys spec,
/// section 2: what the keyboard's cursor moves through).
pub fn shown_rows(groups: &[(Attention, Vec<Entry>)], folded: &Folded, filtering: bool) -> Vec<RowKey> {
    groups.iter().filter(|(kind, _)| !is_folded(folded, *kind, filtering)).flat_map(|(_, rows)| rows.iter().map(row_key)).collect()
}

/// Where the keyboard's cursor goes from `at` by `delta` rows: it stops at either end, and from a
/// row no longer shown (or none) it starts on the first.
pub fn cursor_step(rows: &[RowKey], at: Option<RowKey>, delta: isize) -> Option<RowKey> {
    let i = at.and_then(|a| rows.iter().position(|r| *r == a));
    let next = match i {
        Some(i) => (i as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
        None => 0,
    };
    rows.get(next).copied()
}

/// The session after (or before, with -1) `current` among the live ones listed, wrapping; the
/// first when `current` is not among them (Cmd+Option+Down and Up).
pub fn cycle_session(rows: &[RowKey], current: PageKey, delta: isize) -> Option<PageKey> {
    let live: Vec<PageKey> = rows.iter().filter_map(|r| if let RowKey::Live(p) = r { Some(*p) } else { None }).collect();
    let n = live.len() as isize;
    if n == 0 {
        return None;
    }
    let next = match live.iter().position(|p| *p == current) {
        Some(i) => (i as isize + delta).rem_euclid(n),
        None => 0,
    };
    Some(live[next as usize])
}

/// What a row's right-click menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Rename,
    End,
    /// A closed session: start it again, or forget it.
    Resume(usize),
    Forget(usize),
    /// To another section, from "Move to".
    Move(crate::attention::MoveTo),
    /// A reminder at this time, with this repeat, from "Remind me in".
    Remind(u64, reminder::Repeat),
    /// The reminder ended: the session is listed as its terminals put it again.
    Unschedule,
}

/// A row of a session's menu: something it does, or "Move to", which opens the sections the
/// session can be moved to beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Do(RowAction),
    MoveTo,
    RemindMe,
}

/// A card a session's menu opens beside a row of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    Move = 0,
    Remind = 1,
}

/// A session row's menu: "Move to" when there is a section to move it to (`can_move`), then
/// "Remind me in", Unschedule while it has a reminder (`scheduled`), Rename and End, for a shell
/// that still runs.
pub fn menu_items(can_move: bool, alive: bool, scheduled: bool) -> Vec<(&'static str, MenuEntry)> {
    let mut v = Vec::new();
    if can_move {
        v.push(("Move to", MenuEntry::MoveTo));
    }
    if alive {
        v.push(("Remind me in", MenuEntry::RemindMe));
        if scheduled {
            v.push(("Unschedule", MenuEntry::Do(RowAction::Unschedule)));
        }
        v.push(("Rename", MenuEntry::Do(RowAction::Rename)));
        v.push(("End session", MenuEntry::Do(RowAction::End)));
    }
    v
}

/// How long until `until`, as a Later row says it in place of an age: "in 5 min", "in 3 h".
pub fn until_text(until: u64, now: u64) -> String {
    let secs = until.saturating_sub(now) / 1000;
    match secs {
        0..=59 => "in 1 min".to_string(),
        60..=3599 => format!("in {} min", secs.div_ceil(60)),
        3600..=86_399 => format!("in {} h", secs.div_ceil(3600)),
        _ => format!("in {} d", secs.div_ceil(86_400)),
    }
}

/// `hour:min` local time, `days` days after `now`'s day (milliseconds since the Unix epoch), and
/// the weekday of `now` (0 Sunday). None when the clock cannot be read.
fn local_at(now: u64, days: i32, hour: i32, min: i32) -> Option<(u64, i32)> {
    let secs = (now / 1000) as libc::time_t;
    // SAFETY: `localtime_r` and `mktime` read and write only the two values passed, which live
    // for the calls; a zeroed `tm` is a valid value for `localtime_r` to fill.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        let weekday = tm.tm_wday;
        tm.tm_mday += days;
        tm.tm_hour = hour;
        tm.tm_min = min;
        tm.tm_sec = 0;
        // Let `mktime` decide whether that day is in daylight time.
        tm.tm_isdst = -1;
        let at = libc::mktime(&mut tm);
        (at >= 0).then_some((at as u64 * 1000, weekday))
    }
}

/// Nine in the morning, local time, on the day after `now` (milliseconds since the Unix epoch).
pub fn tomorrow_at_nine(now: u64) -> u64 {
    match local_at(now, 1, 9, 0) {
        Some((at, _)) if at > now => at,
        _ => now + 24 * 3_600_000,
    }
}

/// Nine in the morning, local time, on the Monday after `now`: a week on from a Monday.
pub fn next_week(now: u64) -> u64 {
    let weekday = local_at(now, 0, 0, 0).map_or(1, |(_, w)| w);
    let days = match (8 - weekday) % 7 {
        0 => 7,
        d => d,
    };
    match local_at(now, days, 9, 0) {
        Some((at, _)) if at > now => at,
        _ => now + 7 * 24 * 3_600_000,
    }
}

/// The row menu's "Remind me in" times, as Slack offers them: its label, a hint of when it is
/// (blank for the ones that say it), and the time.
pub fn remind_options(now: u64) -> Vec<(&'static str, &'static str, u64)> {
    vec![
        ("In 20 minutes", "", now + 20 * 60_000),
        ("In 1 hour", "", now + 3_600_000),
        ("In 3 hours", "", now + 3 * 3_600_000),
        ("Tomorrow", "9:00", tomorrow_at_nine(now)),
        ("Next week", "Mon 9:00", next_week(now)),
    ]
}

/// The window's sessions: one entry for each page with a terminal, in the order the pages were
/// made, then the closed ones kept for resuming.
pub fn entries(root: &Root, now: u64) -> Vec<Entry> {
    let tree = &root.tree;
    let mut out = Vec::new();
    for page in tree.page_keys() {
        let terms: Vec<Term> = tree
            .page_tabs(page)
            .into_iter()
            .filter_map(|(pane, idx, tab)| {
                let Tab::Terminal { session, cwd } = tab else { return None };
                let id = tree.tab_id(pane, idx)?;
                let label = root.sessions.labels.get(session);
                let typing = root.tab_rename.as_ref().filter(|r| r.tab == id).map(|r| r.edit.text.as_str());
                let name = shown_name(tree.tab_name(id), typing, label.map(|l| l.title.as_str()), &root.shell_name);
                let folder = || {
                    let dir = cwd.as_deref().unwrap_or(&root.project);
                    dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned())
                };
                let marks = root.sessions.marks.get(session).copied().unwrap_or_default();
                let line = root
                    .sessions
                    .note_of(*session)
                    .map(str::to_string)
                    .or_else(|| label.and_then(|l| title_line(&l.title, &name, &root.shell_name)))
                    .unwrap_or_else(|| format!("in {}", folder()));
                Some(Term { session: *session, tab: id, pane, idx, name, line, attention: root.sessions.attention_of(*session, now), alive: label.is_none_or(|l| l.alive), since: marks.since })
            })
            .collect();
        let focused = tree.page_focused(page).and_then(|p| tree.pane(p)).and_then(|p| match p.tabs.get(p.active) {
            Some(Tab::Terminal { session, .. }) => Some(*session),
            _ => None,
        });
        out.extend(page_entry(page, terms, focused, root.reminders.get(&page), now));
    }
    out.extend(root.ended.iter().enumerate().map(|(i, e)| Entry {
        ended: Some(i),
        page: 0,
        session: SessionId(e.session),
        pane: 0,
        idx: 0,
        name: e.name.clone(),
        line: e.line.clone(),
        names: Vec::new(),
        count: 1,
        attention: Attention::Done,
        group: Attention::Done,
        alive: false,
        since: e.ended,
        scheduled_until: None,
        repeats: false,
        reminded: false,
    }));
    out
}

/// The list's rows under their groups. `width` is the list's own, for the lines' ellipses.
/// `report_end`: the list is the sidebar's whole column (the Inbox), so it says where its last
/// row ends, for an effect that dims what lies under the names.
pub fn render(root: &mut Root, width: f32, report_end: bool, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let now = now_ms();
    // The filter narrows the list, and opens Done so a closed session can be found.
    let query = root.session_filter.text.clone();
    let filtering = !query.trim().is_empty();
    let groups = grouped(matching(hold(entries(root, now), root.held), &query));
    let weak = cx.weak_entity();
    let (highlights, hovered) = (root.effect_highlights(), root.effect_hover);
    let shown = root.tree.page;
    let breath = root.breath.alpha(root.frame_now, motion::reduce_motion());
    let (selected_bg, hover_bg, dim) = (c.tab_active.hsla(), c.tab_inactive.hsla(), c.text_dim.hsla());
    // The keyboard's cursor (Cmd+0), drawn as a ring while the filter has the keys.
    let cursor = root.session_cursor.filter(|_| root.session_filter_focused);
    let ring = c.focus_ring.hsla();

    let pad = 10.0 + style.sidebar_pad;
    let mut list = div().id("session-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
    if filtering && groups.is_empty() {
        list = list.child(div().px(px(pad)).py(px(8.0)).text_size(px(style.ui(12.0))).text_color(dim).child("No session matches."));
    }
    for (kind, rows) in groups {
        // While the list has the keys (Cmd+0) every group is open, as a typed filter opens them,
        // so the ring reaches a Quiet or Done session too.
        let folded = is_folded(&root.sessions_folded, kind, filtering || root.session_filter_focused);
        let heading = div()
            .id(("session-group", kind as usize))
            .h(px(GROUP_H))
            .flex_none()
            .pl(px(pad))
            .pr(px(10.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_size(px(style.ui(11.0)))
            .text_color(dim)
            .child(div().flex_none().w(px(10.0)).child(if folded { "\u{25b8}" } else { "\u{25be}" }))
            // The unread are heavier, and in the text's own colour, so new answers catch the eye.
            .child(if kind == Attention::Unread { heavy(kind.label(), c.text.hsla()).flex_1() } else { div().flex_1().child(kind.label()) })
            .child(div().flex_none().text_color(if kind.calls() { c.text.hsla() } else { dim }).child(rows.len().to_string()))
            .cursor_pointer()
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                toggle_fold(&mut root.sessions_folded, kind);
                root.save(cx);
                cx.notify();
            }));
        list = list.child(heading);
        if folded {
            continue;
        }
        for e in rows {
            let mark = TabMark::of(e.attention, e.alive);
            let bold = e.attention.calls();
            let muted = e.attention == Attention::Done;
            // A reminder up says so in place of the age; one pending says when it comes, with a
            // repeat's arrow; a page of several sessions says how many.
            let when = match e.scheduled_until {
                Some(until) => format!("{}{}", if e.repeats { "\u{21bb} " } else { "" }, until_text(until, now)),
                None => attention::age(e.since, now),
            };
            let when = if e.count > 1 { format!("{} sessions{}{}", e.count, if when.is_empty() { "" } else { " \u{b7} " }, when) } else { when };
            let badge = badge(&e);
            let reminded = badge == Badge::Reminder;
            let key = match e.ended {
                Some(i) => RowKey::Ended(i),
                None => RowKey::Live(e.page),
            };
            let page = e.page;
            let name_color = if muted { dim } else { c.text.hsla() };
            let line_color = if bold { c.text.hsla() } else { dim };
            // Under an effect that paints the highlights the row reports a mark, as the
            // Explorer's rows do, and the effect draws it inside its frame; else the app fills it.
            let mark_key = match key {
                RowKey::Live(p) => MARK_LIVE ^ p,
                RowKey::Ended(i) => MARK_ENDED + i as u64,
            };
            let is_selected = key == RowKey::Live(page) && shown == page;
            let state = blocks::HighlightState::of(is_selected, hovered == Some(mark_key)).filter(|_| highlights);
            let row = div()
                .id(match key {
                    RowKey::Live(p) => ("session-row", p as usize),
                    RowKey::Ended(i) => ("ended-row", i),
                })
                .relative()
                .h(px(ROW_H))
                .flex_none()
                .pl(px(pad))
                .pr(px(8.0))
                .flex()
                .gap(px(6.0))
                .when(is_selected && !highlights, |d| d.bg(selected_bg))
                .when(cursor == Some(key), |d| d.child(div().absolute().inset_0().rounded(px(4.0)).border_1().border_color(ring)))
                .when(!highlights, |d| d.hover(move |s| s.bg(hover_bg)))
                .when(highlights, |d| {
                    let w_hover = weak.clone();
                    d.on_hover(move |over: &bool, _window, cx| {
                        let _ = w_hover.update(cx, |root, cx| {
                            root.effect_hover = blocks::hover_after(root.effect_hover, mark_key, *over);
                            cx.notify();
                        });
                    })
                })
                .when_some(state, |d, state| {
                    let w_mark = weak.clone();
                    d.child(
                        gpui::canvas(
                            move |b, _window, cx| {
                                let rect = crate::tree::Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) };
                                let _ = w_mark.update(cx, |root, _| root.effect_marks.push(blocks::Mark { key: mark_key, rect, kind: blocks::Kind::RowHighlight, state }));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                })
                .cursor_pointer()
                // The mark's slot, level with the name.
                .child(div().flex_none().w(px(12.0)).h(px(style.ui(20.0))).flex().items_center().justify_center().children(mark.glyph(&theme, breath)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .line_height(px(style.ui(18.0)))
                                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(name_color).when(bold, |d| d.font_weight(gpui::FontWeight::BOLD)).child(e.name))
                                .when(reminded, |d| d.child(div().flex_none().px(px(5.0)).rounded(px(4.0)).border_1().border_color(c.focus_ring.hsla()).text_size(px(style.ui(10.0))).text_color(c.text.hsla()).child("Reminder")))
                                .when(badge == Badge::Scheduled, |d| d.child(div().flex_none().px(px(5.0)).rounded(px(4.0)).border_1().border_color(c.border.hsla()).text_size(px(style.ui(10.0))).text_color(dim).child("Scheduled")))
                                .when(!reminded && !when.is_empty(), |d| d.child(div().flex_none().text_size(px(style.ui(11.0))).text_color(dim).child(when))),
                        )
                        // A definite width, as the worktree rows give their second line: gpui
                        // keeps a no-wrap text's first measure, and a long line must end in an
                        // ellipsis rather than be clipped.
                        .child(div().w(px((width - pad - 26.0).max(0.0))).line_height(px(style.ui(15.0))).overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(style.ui(11.0))).text_color(line_color).child(e.line)),
                )
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| match key {
                    RowKey::Live(page) => root.show_row(page, cx),
                    RowKey::Ended(i) => root.resume_ended(i, cx),
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                        // The root's mouse-down closes an open menu; this one just opened.
                        cx.stop_propagation();
                        let now = Instant::now();
                        root.context_menu.hide(now);
                        root.wt_menu.hide(now);
                        root.link_menu.hide(now);
                        root.new_tab_menu.hide(now);
                        root.session_submenu = None;
                        root.session_menu.show((key, ev.position), now);
                        cx.notify();
                    }),
                );
            list = list.child(row);
        }
    }
    // Where the last row ends: the names stop there, and the scene below them is not dimmed.
    if report_end {
        list = list.child(crate::sidebar::sidebar_end_marker(cx));
    }
    list
}

/// The Inbox's filter field over its list: a click puts the keys in it, typing narrows the
/// list to the sessions whose name or line holds every word, Enter goes to the first of them
/// and Escape clears it.
pub fn render_filter(root: &mut Root, cx: &mut Context<Root>) -> Div {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let focused = root.session_filter_focused;
    div().flex_none().px(px(8.0 + style.sidebar_pad)).pr(px(8.0)).pb(px(6.0)).child(
        div()
            .id("session-filter")
            .key_context("SessionFilter")
            .track_focus(&root.session_filter_focus)
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused { c.focus_ring.hsla() } else { c.border.hsla() })
            .overflow_hidden()
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|root, _: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    root.sidebar_focused = false;
                    root.wt_focused = false;
                    root.session_filter_focused = true;
                    root.switcher_focused = false;
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|root, ev: &gpui::KeyDownEvent, _window, cx| root.session_filter_key(&ev.keystroke, cx)))
            .child(crate::line_edit::render_with(&root.session_filter, "Filter sessions", &theme, root.ui_family(), style.ui(12.0), focused)),
    )
}

/// The right-click menu of a session's row, while one is open or fading out: its rows, and beside
/// "Move to" or "Remind me in" the card of what that row opens.
pub fn render_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let ((key, at), opacity) = root.session_menu.current(root.frame_now).map(|(m, o)| (*m, o))?;
    let now = now_ms();
    let row = match key {
        RowKey::Live(page) => root.page_row(page),
        RowKey::Ended(_) => None,
    };
    let targets = row.as_ref().map(|e| attention::row_targets(e.attention, e.alive)).unwrap_or_default();
    let items = match key {
        RowKey::Live(page) => row.as_ref().map(|e| menu_items(!targets.is_empty(), e.alive, root.reminders.contains_key(&page))).unwrap_or_default(),
        RowKey::Ended(i) if i < root.ended.len() => vec![("Resume", MenuEntry::Do(RowAction::Resume(i))), ("Remove from the list", MenuEntry::Do(RowAction::Forget(i)))],
        _ => Vec::new(),
    };
    let page = match key {
        RowKey::Live(page) => page,
        RowKey::Ended(_) => 0,
    };
    if items.is_empty() {
        // The session went away: the menu goes too, at once, since nothing is left to draw.
        root.session_menu.clear();
        return None;
    }
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.session_menu.value().is_some();
    let c = &root.theme.chrome;
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let card = |d: Div| {
        d.flex()
            .flex_col()
            .py(px(4.0))
            .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
            .font_family(root.ui_family())
            .text_size(px(style.ui(12.0)))
            .text_color(c.text.hsla())
    };
    let mut m = card(div().absolute().left(at.x).top(at.y - px(motion::lift(opacity, motion::MENU_LIFT))).opacity(opacity).min_w(px(200.0)))
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x14, opacity, cx));
    }
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    let open = if live { root.session_submenu } else { None };
    let weak = cx.weak_entity();
    for (i, (label, entry)) in items.into_iter().enumerate() {
        let sub = match entry {
            MenuEntry::MoveTo => Some(Sub::Move),
            MenuEntry::RemindMe => Some(Sub::Remind),
            MenuEntry::Do(_) => None,
        };
        let row = div()
            .id(("session-menu-row", i))
            .relative()
            .px(px(10.0))
            .py(px(4.0))
            .flex()
            .items_center()
            .child(div().flex_1().child(label))
            .when(sub.is_some(), |d| d.child(div().flex_none().pl(px(12.0)).text_color(c.text_dim.hsla()).child("\u{25b8}")))
            // A row stays lit while what it opens is open beside it.
            .when(sub.is_some() && open == sub, |d| d.bg(hover))
            .when_some(sub, |d, which| {
                let w = weak.clone();
                d.child(
                    gpui::canvas(
                        move |b, _window, cx| {
                            let _ = w.update(cx, |root, _| root.session_submenu_at[which as usize] = Some(b));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            });
        m = m.child(row.when(live, |row| {
            let key = blocks::menu_row_key(0x14, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx)
                .on_hover(cx.listener(move |root, over: &bool, _window, cx| {
                    // Over a row with a card it opens; over another row it closes.
                    if *over && root.session_submenu != sub {
                        root.session_submenu = sub;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                    match entry {
                        MenuEntry::MoveTo | MenuEntry::RemindMe => root.session_submenu = sub,
                        MenuEntry::Do(action) => {
                            root.close_session_menu();
                            root.session_row_action(page, action, window, cx);
                        }
                    }
                    cx.notify();
                }))
        }));
    }
    let mut out = div().absolute().size_full().top_0().left_0().child(m);
    // The card beside the open row, its top level with that row.
    let Some(which) = open else { return Some(out) };
    let Some(anchor) = root.session_submenu_at[which as usize] else { return Some(out) };
    let mut sub = card(div().absolute().left(anchor.origin.x + anchor.size.width).top(anchor.origin.y - px(4.0)).min_w(px(190.0)))
        .occlude()
        .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation());
    let pick = |id: (&'static str, usize), label: &'static str, hint: &'static str, action: RowAction, cx: &mut Context<Root>| {
        let key = blocks::menu_row_key(if id.0 == "session-move-row" { 0x15 } else { 0x16 }, id.1);
        crate::chrome::menu_row_hover(
            div()
                .id(id)
                .px(px(10.0))
                .py(px(4.0))
                .flex()
                .items_center()
                .gap(px(16.0))
                .cursor_pointer()
                .child(div().flex_1().child(label))
                .when(!hint.is_empty(), |d| d.child(div().flex_none().text_color(c.text_dim.hsla()).child(hint))),
            key,
            hovered == Some(key),
            effect_rows,
            hover,
            cx,
        )
        .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
            root.close_session_menu();
            root.session_row_action(page, action, window, cx);
            cx.notify();
        }))
    };
    match which {
        Sub::Move => {
            for (i, (label, to)) in targets.into_iter().enumerate() {
                sub = sub.child(pick(("session-move-row", i), label, "", RowAction::Move(to), cx));
            }
        }
        Sub::Remind => {
            for (i, (label, hint, at)) in remind_options(now).into_iter().enumerate() {
                sub = sub.child(pick(("session-remind-row", i), label, hint, RowAction::Remind(at, reminder::Repeat::Never), cx));
            }
            // Custom: the date and time picker, in the menu's place.
            let key = blocks::menu_row_key(0x16, 9);
            sub = sub.child(
                crate::chrome::menu_row_hover(div().id("session-remind-custom").px(px(10.0)).py(px(4.0)).cursor_pointer().child("Custom\u{2026}"), key, hovered == Some(key), effect_rows, hover, cx)
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| root.open_remind_picker(page, window, cx))),
            );
        }
    }
    out = out.child(sub);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, attention: Attention, since: u64) -> Entry {
        Entry { ended: None, page: id, session: SessionId(id), pane: 1, idx: 0, name: format!("s{id}"), line: String::new(), names: vec![format!("s{id}")], count: 1, attention, group: attention, alive: true, since, scheduled_until: None, repeats: false, reminded: false }
    }

    fn term(id: u64, tab: TabId, attention: Attention) -> Term {
        Term { session: SessionId(id), tab, pane: tab, idx: 0, name: format!("t{id}"), line: format!("line {id}"), attention, alive: true, since: id * 10 }
    }

    #[test]
    fn a_page_is_one_row_listed_by_its_most_urgent_terminal_and_named_for_its_first() {
        let e = page_entry(7, vec![term(2, 20, Attention::Working), term(1, 10, Attention::Quiet), term(3, 30, Attention::NeedsYou)], None, None, 0).unwrap();
        assert_eq!((e.page, e.attention, e.session, e.name.as_str(), e.line.as_str(), e.count), (7, Attention::NeedsYou, SessionId(3), "t1", "line 3", 3));
        assert_eq!(e.since, 30, "its newest change");
        assert_eq!(e.names, ["t1", "t2", "t3"]);
        // Among equals the focused one leads, else the oldest.
        let two = || vec![term(1, 10, Attention::Unread), term(2, 20, Attention::Unread)];
        assert_eq!(page_entry(7, two(), Some(SessionId(2)), None, 0).unwrap().session, SessionId(2));
        assert_eq!(page_entry(7, two(), None, None, 0).unwrap().session, SessionId(1));
        assert_eq!(page_entry(7, Vec::new(), None, None, 0), None, "a page with no terminal has no row");
    }

    #[test]
    fn a_page_s_reminder_lists_it_under_scheduled_and_labels_it_when_it_comes_up() {
        let r = Reminder::new(100, reminder::Repeat::Daily);
        let e = page_entry(7, vec![term(1, 10, Attention::Unanswered)], None, Some(&r), 50).unwrap();
        assert_eq!((e.attention, e.scheduled_until, e.repeats, e.reminded), (Attention::Scheduled, Some(100), true, false));
        let e = page_entry(7, vec![term(1, 10, Attention::Quiet)], None, Some(&r), 100).unwrap();
        assert_eq!((e.attention, e.scheduled_until, e.reminded), (Attention::Unread, None, true));
    }

    #[test]
    fn a_scheduled_session_listed_elsewhere_keeps_a_scheduled_label_wherever_it_goes() {
        let r = Reminder::new(1000, reminder::Repeat::Never);
        let at = |a| page_entry(7, vec![term(1, 10, a)], None, Some(&r), 50).unwrap();
        assert_eq!(badge(&at(Attention::Working)), Badge::Scheduled, "replied to: at work, still scheduled");
        assert_eq!(badge(&at(Attention::Unread)), Badge::Scheduled, "its answer unread");
        assert_eq!(badge(&at(Attention::NeedsYou)), Badge::Scheduled);
        assert_eq!(at(Attention::Unread).scheduled_until, Some(1000), "with its time");
        assert_eq!(badge(&at(Attention::Unanswered)), Badge::None, "under Scheduled the heading says it");
        // Held under Unread after the read, the label stays until it is back.
        let mut held = hold(vec![at(Attention::Unanswered)], Some((7, Attention::Unread)));
        assert_eq!(badge(&held.remove(0)), Badge::Scheduled);
        let came = page_entry(7, vec![term(1, 10, Attention::Quiet)], None, Some(&r), 1000).unwrap();
        assert_eq!(badge(&came), Badge::Reminder);
        let plain = page_entry(7, vec![term(1, 10, Attention::Working)], None, None, 50).unwrap();
        assert_eq!(badge(&plain), Badge::None);
    }

    #[test]
    fn the_filter_finds_a_row_by_any_of_its_terminals_names() {
        let e = page_entry(7, vec![term(1, 10, Attention::Quiet), term(2, 20, Attention::Quiet)], None, None, 0).unwrap();
        assert_eq!(matching(vec![e.clone()], "t2").len(), 1);
        assert_eq!(matching(vec![e], "t9").len(), 0);
    }

    #[test]
    fn the_list_groups_sessions_by_what_they_ask_newest_first_and_leaves_out_empty_groups() {
        let groups = grouped(vec![
            entry(1, Attention::Working, 10),
            entry(2, Attention::Unread, 20),
            entry(3, Attention::NeedsYou, 5),
            entry(4, Attention::Unread, 30),
            entry(5, Attention::Done, 1),
            entry(6, Attention::Unread, 30),
        ]);
        let shape: Vec<(Attention, Vec<u64>)> = groups.iter().map(|(k, rows)| (*k, rows.iter().map(|e| e.session.0).collect())).collect();
        assert_eq!(shape, vec![(Attention::NeedsYou, vec![3]), (Attention::Unread, vec![4, 6, 2]), (Attention::Working, vec![1]), (Attention::Done, vec![5])]);
        assert_eq!(calling(&groups), 4, "needs you and unread");
        assert!(grouped(Vec::new()).is_empty());
    }

    #[test]
    fn the_session_just_chosen_stays_among_the_unread_until_another_is_chosen() {
        let list = vec![entry(1, Attention::Unanswered, 10), entry(2, Attention::Unread, 20), entry(3, Attention::Unanswered, 5)];
        let groups = |held| grouped(hold(list.clone(), held)).into_iter().map(|(k, rows)| (k, rows.iter().map(|e| e.session.0).collect::<Vec<_>>())).collect::<Vec<_>>();
        // Session 1 was unread when chosen: it stays there, read, among the unread.
        assert_eq!(groups(Some((1, Attention::Unread))), vec![(Attention::Unread, vec![2, 1]), (Attention::Unanswered, vec![3])]);
        assert_eq!(hold(list.clone(), Some((1, Attention::Unread)))[0].attention, Attention::Unanswered, "its mark says it was read");
        // One that was read already, or nothing held: each where it stands.
        assert_eq!(groups(Some((3, Attention::Unanswered))), vec![(Attention::Unread, vec![2]), (Attention::Unanswered, vec![1, 3])]);
        // A reminder looked at, gone back to wait, stays too; one at work does not.
        let back = vec![entry(1, Attention::Scheduled, 10), entry(2, Attention::Working, 20)];
        assert_eq!(hold(back.clone(), Some((1, Attention::Unread)))[0].group, Attention::Unread);
        assert_eq!(hold(back, Some((2, Attention::Unread)))[1].group, Attention::Working);
        assert_eq!(groups(None), vec![(Attention::Unread, vec![2]), (Attention::Unanswered, vec![1, 3])]);
    }

    #[test]
    fn a_row_shows_the_name_being_typed_while_its_session_is_renamed() {
        assert_eq!(shown_name(Some("IDE"), None, Some("✳ Topic"), "zsh"), "IDE");
        assert_eq!(shown_name(Some("IDE"), Some("Game 2"), Some("✳ Topic"), "zsh"), "Game 2", "the typing, live");
        assert_eq!(shown_name(Some("IDE"), Some("  "), Some("claude"), "zsh"), "claude", "blank gives the automatic name back");
        assert_eq!(shown_name(None, None, None, "zsh"), "zsh");
        assert_eq!(shown_name(None, None, Some(""), "zsh"), "zsh");
    }

    #[test]
    fn a_row_s_second_line_is_what_its_terminal_title_adds_to_its_name() {
        assert_eq!(title_line("✳ Arkos layouts", "IDE", "zsh").as_deref(), Some("Arkos layouts"));
        assert_eq!(title_line("⠂ Fix the save system", "Game 2", "zsh").as_deref(), Some("Fix the save system"));
        assert_eq!(title_line("IDE", "IDE", "zsh"), None, "the name again says nothing");
        assert_eq!(title_line("zsh", "Film", "zsh"), None);
        assert_eq!(title_line("  ✳  ", "Film", "zsh"), None);
    }

    #[test]
    fn the_filter_keeps_the_sessions_whose_name_or_line_holds_every_word() {
        let named = |id: u64, name: &str, line: &str| Entry { name: name.into(), line: line.into(), ..entry(id, Attention::Working, 0) };
        let all = vec![named(1, "Film", "Allow Bash: ffmpeg -i shot-04.mov"), named(2, "Phone Farm", "Which device pool should I use?"), named(3, "Game 2", "The save system passes all 14 tests.")];
        let ids = |q: &str| matching(all.clone(), q).iter().map(|e| e.session.0).collect::<Vec<_>>();
        assert_eq!(ids(""), [1, 2, 3]);
        assert_eq!(ids("   "), [1, 2, 3]);
        assert_eq!(ids("farm"), [2]);
        assert_eq!(ids("FFMPEG"), [1], "a line matches too, whatever the case");
        assert_eq!(ids("game tests"), [3], "every word, in any order and from either line");
        assert_eq!(ids("game ffmpeg"), Vec::<u64>::new());
    }

    #[test]
    fn every_group_folds_under_its_heading_quiet_and_done_at_first_and_a_filter_opens_them() {
        let mut folded: Folded = Attention::FOLDED_AT_FIRST.into_iter().collect();
        let shut = |f: &Folded| Attention::ALL.into_iter().filter(|k| is_folded(f, *k, false)).collect::<Vec<_>>();
        assert_eq!(shut(&folded), [Attention::Quiet, Attention::Done]);
        toggle_fold(&mut folded, Attention::Working);
        toggle_fold(&mut folded, Attention::Done);
        assert_eq!(shut(&folded), [Attention::Working, Attention::Quiet]);
        toggle_fold(&mut folded, Attention::Working);
        assert_eq!(shut(&folded), [Attention::Quiet]);
        assert!(Attention::ALL.iter().all(|k| !is_folded(&folded, *k, true)), "a filter shows what it finds");
        // Saved by the sections' names.
        assert_eq!(serde_json::to_string(&Attention::NeedsYou).unwrap(), "\"needs_you\"");
    }

    #[test]
    fn the_keyboard_cursor_moves_through_the_rows_shown_and_stops_at_either_end() {
        let groups = grouped(vec![entry(1, Attention::NeedsYou, 9), entry(2, Attention::Working, 8), entry(3, Attention::Quiet, 7), entry(4, Attention::Working, 6)]);
        let folded: Folded = [Attention::Quiet].into_iter().collect();
        let rows = shown_rows(&groups, &folded, false);
        assert!(!rows.contains(&RowKey::Live(3)), "a folded group's rows are skipped");
        assert_eq!(rows.len(), 3);
        assert_eq!(shown_rows(&groups, &folded, true).len(), 4, "a filter shows every row it finds");
        assert_eq!(cursor_step(&rows, None, 1), Some(rows[0]), "from nowhere, the first row");
        assert_eq!(cursor_step(&rows, Some(RowKey::Live(3)), 1), Some(rows[0]), "from a row gone, the first row");
        assert_eq!(cursor_step(&rows, Some(rows[0]), 1), Some(rows[1]));
        assert_eq!(cursor_step(&rows, Some(rows[0]), -1), Some(rows[0]), "stops at the top");
        assert_eq!(cursor_step(&rows, Some(rows[2]), 1), Some(rows[2]), "stops at the bottom");
        assert_eq!(cursor_step(&[], None, 1), None);
    }

    #[test]
    fn the_next_and_previous_session_follow_the_list_and_wrap() {
        let rows = [RowKey::Live(4), RowKey::Live(2), RowKey::Ended(0), RowKey::Live(7)];
        assert_eq!(cycle_session(&rows, 4, 1), Some(2));
        assert_eq!(cycle_session(&rows, 2, 1), Some(7), "a closed session is no stop");
        assert_eq!(cycle_session(&rows, 7, 1), Some(4), "wraps to the top");
        assert_eq!(cycle_session(&rows, 4, -1), Some(7), "and to the bottom");
        assert_eq!(cycle_session(&rows, 9, 1), Some(4), "from a page not listed, the first");
        assert_eq!(cycle_session(&[RowKey::Ended(1)], 1, 1), None);
    }

    #[test]
    fn the_next_session_that_needs_you_is_a_block_first_then_the_unread_then_the_unanswered() {
        let groups = grouped(vec![entry(1, Attention::Working, 9), entry(2, Attention::Unanswered, 8), entry(3, Attention::Unread, 7), entry(4, Attention::NeedsYou, 6)]);
        assert_eq!(next_calling(&groups, None), Some(4));
        assert_eq!(next_calling(&groups, Some(4)), Some(3), "from the block, on to the unread");
        assert_eq!(next_calling(&groups, Some(1)), Some(4));
        let one = grouped(vec![entry(2, Attention::Unanswered, 8), entry(1, Attention::Working, 9)]);
        assert_eq!(next_calling(&one, Some(2)), Some(2), "the only one is where the user is");
        assert_eq!(next_calling(&grouped(vec![entry(1, Attention::Working, 9), entry(5, Attention::Done, 1)]), None), None);
    }

    #[test]
    fn a_row_s_menu_moves_it_to_another_section_renames_it_or_ends_it() {
        let labels = |can_move, alive, scheduled| menu_items(can_move, alive, scheduled).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(true, true, false), ["Move to", "Remind me in", "Rename", "End session"]);
        assert_eq!(labels(false, true, false), ["Remind me in", "Rename", "End session"]);
        assert_eq!(labels(true, true, true), ["Move to", "Remind me in", "Unschedule", "Rename", "End session"], "a scheduled session can be unscheduled");
        assert!(labels(false, false, true).is_empty(), "a shell that ended has nothing left to do here");
        assert_eq!(menu_items(true, true, false)[0].1, MenuEntry::MoveTo);
        assert_eq!(menu_items(true, true, true)[2].1, MenuEntry::Do(RowAction::Unschedule));
    }

    #[test]
    fn a_closed_session_is_kept_once_newest_first_and_its_resume_line_is_quoted() {
        let ended = |session: u64, id: &str, at: u64| Ended { session, name: format!("s{session}"), line: String::new(), cwd: None, resume: vec!["claude".into(), "--resume".into(), id.into()], ended: at };
        let mut list = Vec::new();
        keep_ended(&mut list, ended(1, "a", 10));
        keep_ended(&mut list, ended(2, "b", 20));
        keep_ended(&mut list, ended(3, "a", 30));
        assert_eq!(list.iter().map(|e| e.session).collect::<Vec<_>>(), [3, 2], "the same conversation closed again replaces its entry");
        for i in 0..(MAX_ENDED as u64 + 5) {
            keep_ended(&mut list, ended(100 + i, &format!("c{i}"), 100 + i));
        }
        assert_eq!(list.len(), MAX_ENDED);
        assert_eq!(list[0].session, 100 + MAX_ENDED as u64 + 4, "newest first");
        let line = resume_line(&["env".into(), "CLAUDE_CONFIG_DIR=/Users/me/My Claude".into(), "claude".into(), "--resume".into(), "it's".into()]);
        assert_eq!(String::from_utf8(line).unwrap(), "'env' 'CLAUDE_CONFIG_DIR=/Users/me/My Claude' 'claude' '--resume' 'it'\\''s'\r");
    }

    #[test]
    fn remind_me_in_offers_slack_s_times() {
        let now = 1_794_000_000_000u64;
        let labels: Vec<&str> = remind_options(now).iter().map(|o| o.0).collect();
        assert_eq!(labels, ["In 20 minutes", "In 1 hour", "In 3 hours", "Tomorrow", "Next week"]);
        let at: Vec<u64> = remind_options(now).iter().map(|o| o.2).collect();
        assert_eq!(&at[..3], &[now + 1_200_000, now + 3_600_000, now + 10_800_000]);
        assert!(at.windows(2).all(|w| w[0] < w[1]), "each later than the one before: {at:?}");
        let week = next_week(now);
        assert!(week > now && week - now <= 8 * 24 * 3_600_000 && week - now > 24 * 3_600_000 - 1);
    }

    #[test]
    fn a_scheduled_row_says_how_long_until_it_is_back_and_tomorrow_at_nine_is_the_next_morning() {
        let min = 60_000;
        assert_eq!(until_text(10 * min, 0), "in 10 min");
        assert_eq!(until_text(10 * min + 1000, 0), "in 11 min");
        assert_eq!(until_text(90 * min, 0), "in 2 h");
        assert_eq!(until_text(30_000, 0), "in 1 min");
        assert_eq!(until_text(0, 5), "in 1 min", "a time that has come");
        // Any time of day: the next nine o'clock that is on another day, under 34 hours off.
        let now = 1_794_000_000_000u64;
        let at = tomorrow_at_nine(now);
        assert!(at > now && at - now <= 34 * 3_600_000, "{}", at - now);
        assert_eq!((at / 1000) % 60, 0);
    }
}
