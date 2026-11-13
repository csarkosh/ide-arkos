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

/// Which row a menu or a click is for: a session the window shows, or a closed one kept for
/// resuming, by its place in the kept list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKey {
    Live(SessionId),
    Ended(usize),
}

/// One session of the window as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// A closed session kept for resuming, by its place in the kept list; None for a session
    /// the window shows.
    pub ended: Option<usize>,
    pub session: SessionId,
    pub pane: PaneId,
    pub idx: usize,
    /// The tab's name: the user's own for it, else its program's title.
    pub name: String,
    /// What its agent asks or last said; for a session with nothing said, its folder.
    pub line: String,
    pub attention: Attention,
    /// The group it is listed under: its attention, except for the session the user has just
    /// chosen, which stays among the unread until they choose another (`hold`).
    pub group: Attention,
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
/// clicked does not move from under the pointer.
pub fn hold(mut entries: Vec<Entry>, held: Option<(SessionId, Attention)>) -> Vec<Entry> {
    if let Some((session, Attention::Unread)) = held {
        for e in entries.iter_mut().filter(|e| e.ended.is_none() && e.session == session && e.attention == Attention::Unanswered) {
            e.group = Attention::Unread;
        }
    }
    entries
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
            let text = format!("{} {}", e.name, e.line).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect()
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

/// The list's height: every group's heading, and the rows of each that is open.
pub fn content_height(groups: &[(Attention, Vec<Entry>)], folded: &Folded) -> f32 {
    groups.iter().map(|(kind, rows)| GROUP_H + if folded.contains(kind) { 0.0 } else { rows.len() as f32 * ROW_H }).sum()
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
    /// A closed session: start it again, or forget it.
    Resume(usize),
    Forget(usize),
    /// To another section, from "Move to".
    Move(crate::attention::MoveTo),
}

/// A row of a session's menu: something it does, or "Move to", which opens the sections the
/// session can be moved to beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Do(RowAction),
    MoveTo,
}

/// A session row's menu: "Move to" when there is a section to move it to (`can_move`), then
/// Rename and End, for a shell that still runs.
pub fn menu_items(can_move: bool, alive: bool) -> Vec<(&'static str, MenuEntry)> {
    let mut v = Vec::new();
    if can_move {
        v.push(("Move to", MenuEntry::MoveTo));
    }
    if alive {
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
            let line = root
                .sessions
                .note_of(*session)
                .map(str::to_string)
                .or_else(|| label.and_then(|l| title_line(&l.title, &name, &root.shell_name)))
                .unwrap_or_else(|| format!("in {}", folder()));
            Some(Entry {
                ended: None,
                session: *session,
                pane,
                idx,
                name,
                line,
                attention: root.sessions.attention_of(*session, now),
                group: root.sessions.attention_of(*session, now),
                alive: label.is_none_or(|l| l.alive),
                since: marks.since,
                later_until: marks.later_until,
            })
        })
        .chain(root.ended.iter().enumerate().map(|(i, e)| Entry {
            ended: Some(i),
            session: SessionId(e.session),
            pane: 0,
            idx: 0,
            name: e.name.clone(),
            line: e.line.clone(),
            attention: Attention::Done,
            group: Attention::Done,
            alive: false,
            since: e.ended,
            later_until: None,
        }))
        .collect()
}

/// The list's rows under their groups. `width` is the list's own, for the lines' ellipses.
/// `report_end`: the list is the sidebar's whole column (the Inbox), so it says where its last
/// row ends, for an effect that dims what lies under the names.
pub fn render(root: &mut Root, width: f32, report_end: bool, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let now = now_ms();
    // The Inbox's filter narrows the list, and opens Done so a closed session can be found.
    let query = if root.is_inbox() { root.session_filter.text.clone() } else { String::new() };
    let filtering = !query.trim().is_empty();
    let groups = grouped(matching(hold(entries(root, now), root.held), &query));
    let weak = cx.weak_entity();
    let (highlights, hovered) = (root.effect_highlights(), root.effect_hover);
    let shown = root.focused_session();
    let breath = root.breath.alpha(root.frame_now, motion::reduce_motion());
    let (selected_bg, hover_bg, dim) = (c.tab_active.hsla(), c.tab_inactive.hsla(), c.text_dim.hsla());

    let pad = 10.0 + style.sidebar_pad;
    let mut list = div().id("session-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
    if filtering && groups.is_empty() {
        list = list.child(div().px(px(pad)).py(px(8.0)).text_size(px(style.ui(12.0))).text_color(dim).child("No session matches."));
    }
    for (kind, rows) in groups {
        let folded = is_folded(&root.sessions_folded, kind, filtering);
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
            let when = match (e.attention, e.later_until) {
                (Attention::Later, Some(until)) => until_text(until, now),
                _ => attention::age(e.since, now),
            };
            let key = match e.ended {
                Some(i) => RowKey::Ended(i),
                None => RowKey::Live(e.session),
            };
            let session = e.session;
            let name_color = if muted { dim } else { c.text.hsla() };
            let line_color = if bold { c.text.hsla() } else { dim };
            // Under an effect that paints the highlights the row reports a mark, as the
            // Explorer's rows do, and the effect draws it inside its frame; else the app fills it.
            let mark_key = match key {
                RowKey::Live(s) => MARK_LIVE ^ s.0,
                RowKey::Ended(i) => MARK_ENDED + i as u64,
            };
            let is_selected = key == RowKey::Live(session) && shown == Some(session);
            let state = blocks::HighlightState::of(is_selected, hovered == Some(mark_key)).filter(|_| highlights);
            let row = div()
                .id(match key {
                    RowKey::Live(s) => ("session-row", s.0 as usize),
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
                                .when(!when.is_empty(), |d| d.child(div().flex_none().text_size(px(style.ui(11.0))).text_color(dim).child(when))),
                        )
                        // A definite width, as the worktree rows give their second line: gpui
                        // keeps a no-wrap text's first measure, and a long line must end in an
                        // ellipsis rather than be clipped.
                        .child(div().w(px((width - pad - 26.0).max(0.0))).line_height(px(style.ui(15.0))).overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(style.ui(11.0))).text_color(line_color).child(e.line)),
                )
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| match key {
                    RowKey::Live(session) => root.show_session(session, cx),
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
                        root.view_menu.hide(now);
                        root.session_submenu = false;
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
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|root, ev: &gpui::KeyDownEvent, _window, cx| root.session_filter_key(&ev.keystroke, cx)))
            .child(crate::line_edit::render_with(&root.session_filter, "Filter sessions", &theme, root.ui_family(), style.ui(12.0), focused)),
    )
}

/// The right-click menu of a session's row, while one is open or fading out.
pub fn render_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let ((key, at), opacity) = root.session_menu.current(root.frame_now).map(|(m, o)| (*m, o))?;
    let now = now_ms();
    let targets = match key {
        RowKey::Live(session) => root.sessions.move_targets(session, now, tomorrow_at_nine(now)),
        RowKey::Ended(_) => Vec::new(),
    };
    let items = match key {
        RowKey::Live(session) if root.tree.find_session(session).is_some() => {
            let alive = root.sessions.labels.get(&session).is_none_or(|l| l.alive);
            menu_items(!targets.is_empty(), alive)
        }
        RowKey::Ended(i) if i < root.ended.len() => vec![("Resume", MenuEntry::Do(RowAction::Resume(i))), ("Remove from the list", MenuEntry::Do(RowAction::Forget(i)))],
        _ => Vec::new(),
    };
    let session = match key {
        RowKey::Live(session) => session,
        RowKey::Ended(_) => SessionId(0),
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
    let sub_open = live && root.session_submenu;
    let weak = cx.weak_entity();
    for (i, (label, entry)) in items.into_iter().enumerate() {
        let is_move = entry == MenuEntry::MoveTo;
        let row = div()
            .id(("session-menu-row", i))
            .relative()
            .px(px(10.0))
            .py(px(4.0))
            .flex()
            .items_center()
            .child(div().flex_1().child(label))
            .when(is_move, |d| d.child(div().flex_none().pl(px(12.0)).text_color(c.text_dim.hsla()).child("\u{25b8}")))
            // "Move to" stays lit while its sections are open beside it.
            .when(is_move && sub_open, |d| d.bg(hover))
            .when(is_move, |d| {
                let w = weak.clone();
                d.child(
                    gpui::canvas(
                        move |b, _window, cx| {
                            let _ = w.update(cx, |root, _| root.session_submenu_at = Some(b));
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
                    // Over "Move to" its sections open; over another row they close.
                    if *over && root.session_submenu != is_move {
                        root.session_submenu = is_move;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                    match entry {
                        MenuEntry::MoveTo => root.session_submenu = true,
                        MenuEntry::Do(action) => {
                            root.session_menu.hide(Instant::now());
                            root.session_submenu = false;
                            root.session_row_action(session, action, window, cx);
                        }
                    }
                    cx.notify();
                }))
        }));
    }
    let mut out = div().absolute().size_full().top_0().left_0().child(m);
    // The sections, in a card beside "Move to", its top level with that row.
    if let (true, Some(anchor)) = (sub_open && !targets.is_empty(), root.session_submenu_at) {
        let mut sub = div()
            .absolute()
            .left(anchor.origin.x + anchor.size.width)
            .top(anchor.origin.y - px(4.0))
            .min_w(px(180.0))
            .flex()
            .flex_col()
            .py(px(4.0))
            .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
            .font_family(root.ui_family())
            .text_size(px(style.ui(12.0)))
            .text_color(c.text.hsla())
            .occlude()
            .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation());
        for (i, (label, to)) in targets.into_iter().enumerate() {
            let key = blocks::menu_row_key(0x15, i);
            sub = sub.child(
                crate::chrome::menu_row_hover(div().id(("session-move-row", i)).px(px(10.0)).py(px(4.0)).cursor_pointer().child(label), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(
                    move |root, _ev: &ClickEvent, window, cx| {
                        root.session_menu.hide(Instant::now());
                        root.session_submenu = false;
                        root.session_row_action(session, RowAction::Move(to), window, cx);
                        cx.notify();
                    },
                )),
            );
        }
        out = out.child(sub);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, attention: Attention, since: u64) -> Entry {
        Entry { ended: None, session: SessionId(id), pane: 1, idx: 0, name: format!("s{id}"), line: String::new(), attention, group: attention, alive: true, since, later_until: None }
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
        let first: Folded = Attention::FOLDED_AT_FIRST.into_iter().collect();
        assert_eq!(content_height(&groups, &first), 4.0 * GROUP_H + 5.0 * ROW_H, "Done is folded");
        assert_eq!(content_height(&groups, &Folded::new()), 4.0 * GROUP_H + 6.0 * ROW_H);
        assert!(grouped(Vec::new()).is_empty());
    }

    #[test]
    fn the_session_just_chosen_stays_among_the_unread_until_another_is_chosen() {
        let list = vec![entry(1, Attention::Unanswered, 10), entry(2, Attention::Unread, 20), entry(3, Attention::Unanswered, 5)];
        let groups = |held| grouped(hold(list.clone(), held)).into_iter().map(|(k, rows)| (k, rows.iter().map(|e| e.session.0).collect::<Vec<_>>())).collect::<Vec<_>>();
        // Session 1 was unread when chosen: it stays there, read, among the unread.
        assert_eq!(groups(Some((SessionId(1), Attention::Unread))), vec![(Attention::Unread, vec![2, 1]), (Attention::Unanswered, vec![3])]);
        assert_eq!(hold(list.clone(), Some((SessionId(1), Attention::Unread)))[0].attention, Attention::Unanswered, "its mark says it was read");
        // One that was read already, or nothing held: each where it stands.
        assert_eq!(groups(Some((SessionId(3), Attention::Unanswered))), vec![(Attention::Unread, vec![2]), (Attention::Unanswered, vec![1, 3])]);
        assert_eq!(groups(None), vec![(Attention::Unread, vec![2]), (Attention::Unanswered, vec![1, 3])]);
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
    fn a_row_s_menu_moves_it_to_another_section_renames_it_or_ends_it() {
        let labels = |can_move, alive| menu_items(can_move, alive).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(true, true), ["Move to", "Rename", "End session"]);
        assert_eq!(labels(false, true), ["Rename", "End session"], "nowhere to move it: a blocked session");
        assert!(labels(false, false).is_empty(), "a shell that ended has nothing left to do here");
        assert_eq!(menu_items(true, true)[0].1, MenuEntry::MoveTo);
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
