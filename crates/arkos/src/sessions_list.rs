//! The window's sessions as a list sorted by what they ask of the user (layouts spec, "The
//! session list"): the sidebar's Sessions section in the Grid, and the Inbox's own list. The
//! grouping, the menu's items and the times are pure and tested; the rows read the root.
use crate::attention::{self, Attention};
use crate::effects::blocks;
use crate::motion;
use crate::pane_view::TabMark;
use crate::sessions::now_ms;
use crate::tree::{PaneId, Tab};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful};
use protocol::SessionId;
use std::time::Instant;

/// A session's row: two lines, its name and age over its line.
pub const ROW_H: f32 = 40.0;
/// A group's heading.
pub const GROUP_H: f32 = 20.0;

/// One session of the window as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub session: SessionId,
    pub pane: PaneId,
    pub idx: usize,
    /// The tab's name: the user's own for it, else its program's title.
    pub name: String,
    /// What its agent asks or last said; for a session with nothing said, its folder.
    pub line: String,
    pub attention: Attention,
    pub alive: bool,
    /// When its state last changed, in milliseconds since the Unix epoch; 0, unknown.
    pub since: u64,
    /// The time it is put off until, while it is.
    pub later_until: Option<u64>,
}

/// The entries under their groups, in `Attention::ALL`'s order, a group with nothing in it left
/// out. Within a group the newest change is first, and sessions of the same moment keep the
/// order they came in (the tabs' own), so the list does not shuffle on its own.
pub fn grouped(entries: Vec<Entry>) -> Vec<(Attention, Vec<Entry>)> {
    let mut groups: Vec<(Attention, Vec<Entry>)> = Vec::new();
    for kind in Attention::ALL {
        let mut rows: Vec<Entry> = entries.iter().filter(|e| e.attention == kind).cloned().collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| b.since.cmp(&a.since));
        groups.push((kind, rows));
    }
    groups
}

/// The session to go to for "the next session that needs you": the first of Needs you, then of
/// Unread, then of Unanswered that is not `current`, and `current` itself when it is the only
/// one. None when no session is the user's move.
pub fn next_calling(groups: &[(Attention, Vec<Entry>)], current: Option<SessionId>) -> Option<SessionId> {
    let mine = |kind: &Attention| matches!(kind, Attention::NeedsYou | Attention::Unread | Attention::Unanswered);
    let all: Vec<SessionId> = groups.iter().filter(|(kind, _)| mine(kind)).flat_map(|(_, rows)| rows.iter().map(|e| e.session)).collect();
    all.iter().copied().find(|s| Some(*s) != current).or(all.first().copied())
}

/// How many sessions call for the user now: the count a Dock badge and the section's header show.
pub fn calling(groups: &[(Attention, Vec<Entry>)]) -> usize {
    groups.iter().filter(|(kind, _)| kind.calls()).map(|(_, rows)| rows.len()).sum()
}

/// The list's height: every group's heading and rows, the Done group's rows only while it is open.
pub fn content_height(groups: &[(Attention, Vec<Entry>)], done_open: bool) -> f32 {
    groups.iter().map(|(kind, rows)| GROUP_H + if *kind == Attention::Done && !done_open { 0.0 } else { rows.len() as f32 * ROW_H }).sum()
}

/// What a row's right-click menu does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Seen(bool),
    Done(bool),
    /// Put off for this many milliseconds, or until tomorrow at nine.
    LaterFor(u64),
    LaterTomorrow,
    BackToList,
    Rename,
    End,
}

/// A session row's menu, by where it stands: reading marks only where there is something to
/// read, putting off only what waits, and End last, for a shell that still runs.
pub fn menu_items(attention: Attention, alive: bool) -> Vec<(&'static str, RowAction)> {
    let mut v = Vec::new();
    match attention {
        Attention::Unread => v.push(("Mark read", RowAction::Seen(true))),
        Attention::Unanswered => v.push(("Mark unread", RowAction::Seen(false))),
        _ => {}
    }
    if matches!(attention, Attention::Unread | Attention::Unanswered) {
        v.push(("Later: in an hour", RowAction::LaterFor(3_600_000)));
        v.push(("Later: tomorrow at 9", RowAction::LaterTomorrow));
    }
    if attention == Attention::Later {
        v.push(("Back to the list", RowAction::BackToList));
    }
    match (attention, alive) {
        (Attention::Done, true) => v.push(("Not done", RowAction::Done(false))),
        // A block cannot be put away: it comes straight back.
        (Attention::NeedsYou, _) | (_, false) => {}
        _ => v.push(("Done", RowAction::Done(true))),
    }
    if alive {
        v.push(("Rename", RowAction::Rename));
        v.push(("End session", RowAction::End));
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

/// Nine in the morning, local time, on the day after `now` (milliseconds since the Unix epoch).
pub fn tomorrow_at_nine(now: u64) -> u64 {
    let secs = (now / 1000) as libc::time_t;
    // SAFETY: `localtime_r` and `mktime` read and write only the two values passed, which live
    // for the calls; a zeroed `tm` is a valid value for `localtime_r` to fill.
    let at = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return now + 24 * 3_600_000;
        }
        tm.tm_mday += 1;
        tm.tm_hour = 9;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        // Let `mktime` decide whether that morning is in daylight time.
        tm.tm_isdst = -1;
        libc::mktime(&mut tm)
    };
    if at <= secs {
        now + 24 * 3_600_000
    } else {
        at as u64 * 1000
    }
}

/// The window's sessions, one entry for each terminal tab, in the tabs' own order.
pub fn entries(root: &Root, now: u64) -> Vec<Entry> {
    root.tree
        .tabs()
        .into_iter()
        .filter_map(|(pane, idx, tab)| {
            let Tab::Terminal { session, cwd } = tab else { return None };
            let label = root.sessions.labels.get(session);
            let named = root.tree.tab_id(pane, idx).and_then(|id| root.tree.tab_name(id)).map(str::to_string);
            let name = named.or_else(|| label.map(|l| l.title.clone()).filter(|t| !t.is_empty())).unwrap_or_else(|| root.shell_name.clone());
            let folder = || {
                let dir = cwd.as_deref().unwrap_or(&root.project);
                dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned())
            };
            let marks = root.sessions.marks.get(session).copied().unwrap_or_default();
            Some(Entry {
                session: *session,
                pane,
                idx,
                name,
                line: root.sessions.note_of(*session).map(str::to_string).unwrap_or_else(folder),
                attention: root.sessions.attention_of(*session, now),
                alive: label.is_none_or(|l| l.alive),
                since: marks.since,
                later_until: marks.later_until,
            })
        })
        .collect()
}

/// The list's rows under their groups. `width` is the list's own, for the lines' ellipses.
pub fn render(root: &mut Root, width: f32, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let now = now_ms();
    let groups = grouped(entries(root, now));
    let shown = root.focused_session();
    let breath = root.breath.alpha(root.frame_now, motion::reduce_motion());
    let (selected_bg, hover_bg, dim) = (c.tab_active.hsla(), c.tab_inactive.hsla(), c.text_dim.hsla());
    let done_open = root.sessions_done_open;
    let pad = 10.0 + style.sidebar_pad;
    let mut list = div().id("session-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
    for (kind, rows) in groups {
        let folded = kind == Attention::Done && !done_open;
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
            .child(div().flex_1().child(if kind == Attention::Done { format!("{} {}", if folded { "\u{25b8}" } else { "\u{25be}" }, kind.label()) } else { kind.label().to_string() }))
            .child(div().flex_none().text_color(if kind.calls() { c.text.hsla() } else { dim }).child(rows.len().to_string()))
            .when(kind == Attention::Done, |d| {
                d.cursor_pointer().on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                    root.sessions_done_open = !root.sessions_done_open;
                    cx.notify();
                }))
            });
        list = list.child(heading);
        if folded {
            continue;
        }
        for e in rows {
            let mark = TabMark::of(e.attention, e.alive);
            let bold = e.attention.calls();
            let muted = e.attention == Attention::Done;
            let when = match (e.attention, e.later_until) {
                (Attention::Later, Some(until)) => until_text(until, now),
                _ => attention::age(e.since, now),
            };
            let (session, pane, idx) = (e.session, e.pane, e.idx);
            let name_color = if muted { dim } else { c.text.hsla() };
            let line_color = if bold { c.text.hsla() } else { dim };
            let row = div()
                .id(("session-row", session.0 as usize))
                .h(px(ROW_H))
                .flex_none()
                .pl(px(pad))
                .pr(px(8.0))
                .flex()
                .gap(px(6.0))
                .when(shown == Some(session), |d| d.bg(selected_bg))
                .hover(move |s| s.bg(hover_bg))
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
                                .when(!when.is_empty(), |d| d.child(div().flex_none().text_size(px(style.ui(11.0))).text_color(dim).child(when))),
                        )
                        // A definite width, as the worktree rows give their second line: gpui
                        // keeps a no-wrap text's first measure, and a long line must end in an
                        // ellipsis rather than be clipped.
                        .child(div().w(px((width - pad - 26.0).max(0.0))).line_height(px(style.ui(15.0))).overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(style.ui(11.0))).text_color(line_color).child(e.line)),
                )
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.show_session(session, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                        // The root's mouse-down closes an open menu; this one just opened.
                        cx.stop_propagation();
                        let now = Instant::now();
                        root.context_menu.hide(now);
                        root.wt_menu.hide(now);
                        root.link_menu.hide(now);
                        root.view_menu.hide(now);
                        root.session_menu.show((session, ev.position), now);
                        cx.notify();
                    }),
                );
            let _ = (pane, idx);
            list = list.child(row);
        }
    }
    list
}

/// The right-click menu of a session's row, while one is open or fading out.
pub fn render_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let ((session, at), opacity) = root.session_menu.current(root.frame_now).map(|(m, o)| (*m, o))?;
    let alive = root.sessions.labels.get(&session).is_none_or(|l| l.alive);
    let shown = root.tree.find_session(session).is_some();
    let items = if shown { menu_items(root.sessions.attention_of(session, now_ms()), alive) } else { Vec::new() };
    if items.is_empty() {
        // The session went away: the menu goes too, at once, since nothing is left to draw.
        root.session_menu.clear();
        return None;
    }
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.session_menu.value().is_some();
    let c = &root.theme.chrome;
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let mut m = div()
        .absolute()
        .left(at.x)
        .top(at.y - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(200.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(style.ui(12.0)))
        .text_color(c.text.hsla())
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x14, opacity, cx));
    }
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    for (i, (label, action)) in items.into_iter().enumerate() {
        m = m.child(div().id(("session-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label).when(live, |row| {
            let key = blocks::menu_row_key(0x14, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                root.session_menu.hide(Instant::now());
                root.session_row_action(session, action, window, cx);
                cx.notify();
            }))
        }));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, attention: Attention, since: u64) -> Entry {
        Entry { session: SessionId(id), pane: 1, idx: 0, name: format!("s{id}"), line: String::new(), attention, alive: true, since, later_until: None }
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
        assert_eq!(content_height(&groups, false), 4.0 * GROUP_H + 5.0 * ROW_H, "Done is folded");
        assert_eq!(content_height(&groups, true), 4.0 * GROUP_H + 6.0 * ROW_H);
        assert!(grouped(Vec::new()).is_empty());
    }

    #[test]
    fn the_next_session_that_needs_you_is_a_block_first_then_the_unread_then_the_unanswered() {
        let groups = grouped(vec![entry(1, Attention::Working, 9), entry(2, Attention::Unanswered, 8), entry(3, Attention::Unread, 7), entry(4, Attention::NeedsYou, 6)]);
        assert_eq!(next_calling(&groups, None), Some(SessionId(4)));
        assert_eq!(next_calling(&groups, Some(SessionId(4))), Some(SessionId(3)), "from the block, on to the unread");
        assert_eq!(next_calling(&groups, Some(SessionId(1))), Some(SessionId(4)));
        let one = grouped(vec![entry(2, Attention::Unanswered, 8), entry(1, Attention::Working, 9)]);
        assert_eq!(next_calling(&one, Some(SessionId(2))), Some(SessionId(2)), "the only one is where the user is");
        assert_eq!(next_calling(&grouped(vec![entry(1, Attention::Working, 9), entry(5, Attention::Done, 1)]), None), None);
    }

    #[test]
    fn a_row_s_menu_offers_what_fits_where_the_session_stands() {
        let labels = |a, alive| menu_items(a, alive).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(Attention::Unread, true), ["Mark read", "Later: in an hour", "Later: tomorrow at 9", "Done", "Rename", "End session"]);
        assert_eq!(labels(Attention::Unanswered, true), ["Mark unread", "Later: in an hour", "Later: tomorrow at 9", "Done", "Rename", "End session"]);
        assert_eq!(labels(Attention::NeedsYou, true), ["Rename", "End session"], "a block cannot be put away");
        assert_eq!(labels(Attention::Working, true), ["Done", "Rename", "End session"]);
        assert_eq!(labels(Attention::Idle, true), ["Done", "Rename", "End session"]);
        assert_eq!(labels(Attention::Later, true), ["Back to the list", "Done", "Rename", "End session"]);
        assert_eq!(labels(Attention::Done, true), ["Not done", "Rename", "End session"]);
        assert!(labels(Attention::Done, false).is_empty(), "a shell that ended has nothing left to do here");
    }

    #[test]
    fn a_later_row_says_how_long_until_it_is_back_and_tomorrow_at_nine_is_the_next_morning() {
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
