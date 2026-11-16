//! "Remind me in", Custom: a date and time picker like Slack's (sessions hold grids spec,
//! section 5). A month of days, a time from a list or typed, and a repeat. The model is pure, so
//! it is tested; `render` draws it over the window and the root routes its keys.
use crate::reminder::{self, Repeat};
use crate::sessions::now_ms;
use crate::tree::PageKey;
use crate::{motion, Root};
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseDownEvent};

/// The minutes between the times the list offers.
pub const STEP: u32 = 15;

/// The picker's state: whose reminder, the month shown, the day, time and repeat chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub page: PageKey,
    /// The month shown: its year and month (1 to 12).
    pub year: i32,
    pub month: u32,
    /// The day chosen.
    pub day: (i32, u32, u32),
    /// The time chosen, in minutes after midnight.
    pub minutes: u32,
    pub repeat: Repeat,
    /// A dropdown open under the time or the repeat.
    pub open: Option<Drop>,
    /// The typed time could not be read.
    pub time_bad: bool,
}

/// Which dropdown is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    Time,
    Repeat,
}

/// The day after `(y, m, d)` moved by `days`, through months and years.
pub fn add_days(day: (i32, u32, u32), days: i32) -> (i32, u32, u32) {
    // Noon, so a daylight change never moves the date.
    let at = reminder::at_local(day.0, day.1, day.2, 12, 0) as i64 + days as i64 * 86_400_000;
    let l = reminder::local(at.max(0) as u64);
    (l.year, l.month, l.day)
}

/// The weekday of a day, 0 Sunday to 6 Saturday.
pub fn weekday(day: (i32, u32, u32)) -> u32 {
    reminder::local(reminder::at_local(day.0, day.1, day.2, 12, 0)).weekday
}

/// A time of day as a list says it: "9:00 AM", "12:30 PM".
pub fn time_label(minutes: u32) -> String {
    let (h, m) = (minutes / 60 % 24, minutes % 60);
    let half = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        h => h,
    };
    format!("{h12}:{m:02} {half}")
}

/// A clock time typed as `14:30`, `9:15am`, `2pm`, `2:30 PM` or `0:05`, in minutes after midnight.
pub fn parse_time(text: &str) -> Option<u32> {
    let t = text.trim().to_lowercase().replace('.', "");
    let (clock, half) = match t.strip_suffix("am") {
        Some(c) => (c.trim().to_string(), Some(false)),
        None => match t.strip_suffix("pm") {
            Some(c) => (c.trim().to_string(), Some(true)),
            None => (t.clone(), None),
        },
    };
    let (h, m) = match clock.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if half.is_some() => (clock.parse::<u32>().ok()?, 0),
        _ => return None,
    };
    let hour = match half {
        Some(pm) if (1..=12).contains(&h) => (h % 12) + if pm { 12 } else { 0 },
        None if h <= 23 => h,
        _ => return None,
    };
    (m <= 59).then_some(hour * 60 + m)
}

/// Every time the list offers: midnight to 11:45 PM, every `STEP` minutes.
pub fn times() -> Vec<u32> {
    (0..24 * 60).step_by(STEP as usize).collect()
}

impl Picker {
    /// The picker for `page` at `now`: on the reminder it has (its first time and its repeat),
    /// else tomorrow at nine, never repeating.
    pub fn new(page: PageKey, now: u64, existing: Option<&reminder::Reminder>) -> Picker {
        let (day, minutes, repeat) = match existing {
            Some(r) => {
                let l = reminder::local(if r.repeat == Repeat::Never { r.at } else { r.anchor });
                ((l.year, l.month, l.day), l.hour * 60 + l.minute, r.repeat)
            }
            None => {
                let l = reminder::local(now);
                (add_days((l.year, l.month, l.day), 1), 9 * 60, Repeat::Never)
            }
        };
        Picker { page, year: day.0, month: day.1, day, minutes, repeat, open: None, time_bad: false }
    }

    /// The month shown as weeks, Sunday first: each slot a day of the month, or none.
    pub fn weeks(&self) -> Vec<[Option<u32>; 7]> {
        let first = weekday((self.year, self.month, 1)) as usize;
        let days = reminder::days_in_month(self.year, self.month) as usize;
        let mut out = Vec::new();
        let mut week = [None; 7];
        for slot in 0..(first + days).div_ceil(7) * 7 {
            let d = slot as i64 - first as i64 + 1;
            week[slot % 7] = (1..=days as i64).contains(&d).then_some(d as u32);
            if slot % 7 == 6 {
                out.push(week);
                week = [None; 7];
            }
        }
        out
    }

    /// The month shown moved by `months`.
    pub fn turn_month(&mut self, months: i32) {
        let m = self.year * 12 + self.month as i32 - 1 + months;
        self.year = m.div_euclid(12);
        self.month = m.rem_euclid(12) as u32 + 1;
    }

    /// Chooses day `d` of the month shown, unless it is before `now`'s day.
    pub fn choose(&mut self, d: u32, now: u64) {
        let day = (self.year, self.month, d);
        if !self.before_today(day, now) {
            self.day = day;
        }
    }

    /// Moves the day chosen by `days` (the arrow keys), never before today, and shows its month.
    pub fn move_day(&mut self, days: i32, now: u64) {
        let day = add_days(self.day, days);
        if !self.before_today(day, now) {
            self.day = day;
            self.year = day.0;
            self.month = day.1;
        }
    }

    /// Whether `day` is before `now`'s day, which cannot be chosen.
    pub fn before_today(&self, day: (i32, u32, u32), now: u64) -> bool {
        let l = reminder::local(now);
        day < (l.year, l.month, l.day)
    }

    /// Whether `day` is `now`'s.
    pub fn is_today(&self, day: (i32, u32, u32), now: u64) -> bool {
        let l = reminder::local(now);
        day == (l.year, l.month, l.day)
    }

    /// The moment chosen, in milliseconds since the Unix epoch.
    pub fn at(&self) -> u64 {
        reminder::at_local(self.day.0, self.day.1, self.day.2, self.minutes / 60, self.minutes % 60)
    }

    /// Whether Save may be pressed: the moment chosen is still to come.
    pub fn can_save(&self, now: u64) -> bool {
        self.at() > now
    }

    /// Sets the time from what is typed; false, and the field says so, when it cannot be read.
    pub fn type_time(&mut self, text: &str) -> bool {
        match parse_time(text) {
            Some(m) => {
                self.minutes = m;
                self.time_bad = false;
                true
            }
            None => {
                self.time_bad = true;
                false
            }
        }
    }

    /// The repeats offered, each named for the day chosen.
    pub fn repeats(&self) -> Vec<(Repeat, String)> {
        let at = self.at();
        [Repeat::Never, Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Monthly].into_iter().map(|r| (r, reminder::repeat_label(r, at))).collect()
    }

    /// What the picker will set, in a line: "Tomorrow at 9:00 AM", "Fri, Nov 20 at 2:30 PM,
    /// every weekday".
    pub fn summary(&self, now: u64) -> String {
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let today = reminder::local(now);
        let today = (today.year, today.month, today.day);
        let day = if self.day == today {
            "Today".to_string()
        } else if self.day == add_days(today, 1) {
            "Tomorrow".to_string()
        } else {
            let year = if self.day.0 != today.0 { format!(", {}", self.day.0) } else { String::new() };
            format!("{}, {} {}{}", DAYS[weekday(self.day) as usize], month_name(self.day.1).get(..3).unwrap_or(""), self.day.2, year)
        };
        let repeat = match self.repeat {
            Repeat::Never => String::new(),
            r => {
                let label = reminder::repeat_label(r, self.at());
                format!(", {}{}", label[..1].to_lowercase(), &label[1..])
            }
        };
        format!("{day} at {}{repeat}", time_label(self.minutes))
    }

    /// The reminder to set: its time and its repeat.
    pub fn reminder(&self) -> (u64, Repeat) {
        (self.at(), self.repeat)
    }
}

/// A month's name, 1 January.
pub fn month_name(month: u32) -> &'static str {
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"].get(month.wrapping_sub(1) as usize).copied().unwrap_or("")
}

/// The card's width: seven day cells and its padding.
const CELL: f32 = 32.0;
const PAD: f32 = 14.0;
/// The label column of the time and repeat rows.
const LABEL_W: f32 = 56.0;

/// The picker over the window while it is open or fading out: a scrim that cancels when clicked,
/// and the card in the middle.
pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let (p, opacity) = root.remind_picker.current(root.frame_now).map(|(p, o)| (p.clone(), o))?;
    let live = root.remind_picker.value().is_some();
    let now = now_ms();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let (text, dim, ring) = (c.text.hsla(), c.text_dim.hsla(), c.focus_ring.hsla());
    let hover = crate::chrome::menu_colors(&theme, style.menus).hover;
    let field_bg = c.tab_active.hsla();
    let ui = |n: f32| px(style.ui(n));
    let w = 7.0 * CELL + 2.0 * PAD;

    // A click that changes the picker and repaints.
    fn on<F: Fn(&mut Picker, u64) + 'static>(f: F, cx: &mut Context<Root>) -> Box<dyn Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App)> {
        Box::new(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
            if let Some(p) = root.remind_picker.value_mut() {
                f(p, now_ms());
            }
            cx.notify();
        }))
    }
    let arrow = |id: &'static str, glyph: &'static str, months: i32, cx: &mut Context<Root>| {
        div().id(id).w(px(24.0)).h(px(24.0)).flex().items_center().justify_center().rounded(px(6.0)).cursor_pointer().hover(move |s| s.bg(hover)).child(glyph).on_click(on(move |p, _| p.turn_month(months), cx))
    };
    let month = div()
        .flex()
        .items_center()
        .child(arrow("remind-prev", "\u{2039}", -1, cx))
        .child(div().flex_1().flex().justify_center().font_weight(gpui::FontWeight::BOLD).child(format!("{} {}", month_name(p.month), p.year)))
        .child(arrow("remind-next", "\u{203a}", 1, cx));
    let heads = div().flex().text_size(ui(11.0)).text_color(dim).children(["S", "M", "T", "W", "T", "F", "S"].into_iter().map(|d| div().w(px(CELL)).flex().justify_center().child(d)));
    let mut days = div().flex().flex_col().gap(px(2.0));
    for (wi, week) in p.weeks().into_iter().enumerate() {
        let mut row = div().flex();
        for (di, slot) in week.into_iter().enumerate() {
            let cell = div().id(("remind-day", wi * 7 + di)).w(px(CELL)).h(px(28.0)).flex().items_center().justify_center().rounded(px(6.0));
            let Some(d) = slot else {
                row = row.child(cell);
                continue;
            };
            let day = (p.year, p.month, d);
            let past = p.before_today(day, now);
            let chosen = day == p.day;
            let cell = cell
                .border_1()
                .border_color(if chosen { ring } else if p.is_today(day, now) { dim } else { gpui::transparent_black() })
                .when(chosen, |d| d.bg(field_bg).font_weight(gpui::FontWeight::BOLD))
                .text_color(if past { dim.alpha(0.45) } else { text })
                .child(d.to_string())
                .when(!past, |cell| cell.cursor_pointer().hover(move |s| s.bg(hover)).on_click(on(move |p, now| {
                    p.choose(d, now);
                    p.open = None;
                }, cx)));
            row = row.child(cell);
        }
        days = days.child(row);
    }
    // A field with a dropdown under it: its label, what it shows, and the list when open.
    let field = |id: &'static str, label: &'static str, shown: gpui::AnyElement, which: Drop, cx: &mut Context<Root>| {
        div()
            .flex()
            .items_center()
            .child(div().w(px(LABEL_W)).text_color(dim).child(label))
            .child(
                div()
                    .id(id)
                    .flex_1()
                    .h(px(26.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if p.open == Some(which) { ring } else { c.border.hsla() })
                    .bg(field_bg)
                    .cursor_pointer()
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(shown))
                    .child(div().flex_none().pl(px(6.0)).text_color(dim).child("\u{25be}"))
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                        root.toggle_remind_drop(which);
                        cx.notify();
                    })),
            )
    };
    let time_shown = match root.remind_time.as_ref() {
        Some(edit) => crate::line_edit::render(edit, "9:00 AM", &theme, root.ui_family(), style.ui(12.0)).into_any_element(),
        None => div().child(time_label(p.minutes)).into_any_element(),
    };
    let time = field("remind-time", "Time", time_shown, Drop::Time, cx);
    let repeat_name = p.repeats().into_iter().find(|(r, _)| *r == p.repeat).map(|(_, n)| n).unwrap_or_default();
    let repeat = field("remind-repeat", "Repeat", div().child(repeat_name).into_any_element(), Drop::Repeat, cx);
    // The open dropdown's list, under its field.
    let list = |rows: Vec<(String, bool, Box<dyn Fn(&mut Picker, u64)>)>, scroll: Option<gpui::ScrollHandle>, cx: &mut Context<Root>| {
        let mut l = div().id("remind-list").absolute().left(px(LABEL_W)).right_0().top(px(28.0)).max_h(px(180.0)).overflow_y_scroll().py(px(4.0)).rounded(px(6.0)).map(|d| crate::chrome::menu_frame(d, &theme, &style)).occlude();
        if let Some(scroll) = scroll {
            l = l.track_scroll(&scroll);
        }
        for (i, (label, on_it, f)) in rows.into_iter().enumerate() {
            let f = std::rc::Rc::new(f);
            l = l.child(div().id(("remind-item", i)).px(px(8.0)).py(px(3.0)).cursor_pointer().hover(move |s| s.bg(hover)).when(on_it, |d| d.bg(hover).font_weight(gpui::FontWeight::BOLD)).child(label).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                if let Some(p) = root.remind_picker.value_mut() {
                    f(p, now_ms());
                    p.open = None;
                }
                root.remind_time = None;
                cx.notify();
            })));
        }
        l
    };
    let time = match p.open {
        Some(Drop::Time) => {
            let rows = times().into_iter().map(|m| (time_label(m), m == p.minutes, Box::new(move |p: &mut Picker, _| p.minutes = m) as Box<dyn Fn(&mut Picker, u64)>)).collect();
            time.relative().child(list(rows, Some(root.remind_scroll.clone()), cx))
        }
        _ => time,
    };
    let repeat = match p.open {
        Some(Drop::Repeat) => {
            let rows = p.repeats().into_iter().map(|(r, n)| (n, r == p.repeat, Box::new(move |p: &mut Picker, _| p.repeat = r) as Box<dyn Fn(&mut Picker, u64)>)).collect();
            repeat.relative().child(list(rows, None, cx))
        }
        _ => repeat,
    };
    let can_save = p.can_save(now);
    let button = |id: &'static str, label: &'static str, filled: bool, enabled: bool| {
        div()
            .id(id)
            .h(px(26.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if filled && enabled { ring } else { c.border.hsla() })
            .when(filled, |d| d.bg(field_bg).font_weight(gpui::FontWeight::BOLD))
            .text_color(if enabled { text } else { dim.alpha(0.6) })
            .when(enabled, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
            .child(label)
    };
    let buttons = div()
        .flex()
        .justify_end()
        .gap(px(8.0))
        .child(button("remind-cancel", "Cancel", false, true).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
            root.close_remind_picker();
            cx.notify();
        })))
        .child(button("remind-save", "Save", true, can_save).on_click(cx.listener(|root, _ev: &ClickEvent, window, cx| root.save_remind_picker(window, cx))));
    let bad = p.time_bad && p.open == Some(Drop::Time);
    let lift = motion::lift(opacity, motion::MENU_LIFT);
    let card = div()
        .id("remind-picker")
        .w(px(w))
        .mt(px(-lift))
        .p(px(PAD))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .rounded(px(10.0))
        .map(|d| crate::chrome::menu_frame(d, &theme, &style))
        .font_family(root.ui_family())
        .text_size(ui(12.0))
        .text_color(text)
        .opacity(opacity)
        .when(live, |d| {
            d.track_focus(&root.remind_focus)
                .key_context("RemindPicker")
                .on_key_down(cx.listener(|root, ev: &gpui::KeyDownEvent, window, cx| root.remind_picker_key(&ev.keystroke, window, cx)))
                .occlude()
                .on_any_mouse_down(cx.listener(|root, _: &MouseDownEvent, window, cx| {
                    window.focus(&root.remind_focus);
                    cx.stop_propagation();
                }))
        })
        .child(div().font_weight(gpui::FontWeight::BOLD).text_size(ui(13.0)).child("Remind me"))
        .child(month)
        .child(heads)
        .child(days)
        .child(time)
        .when(bad, |d| d.child(div().pl(px(LABEL_W)).text_size(ui(11.0)).text_color(dim).child("Try 14:30, 2pm or 9:15am")))
        .child(repeat)
        .child(div().text_size(ui(11.0)).text_color(dim).child(p.summary(now)))
        .child(buttons);
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().alpha(0.18 * opacity))
            .when(live, |d| {
                d.occlude().on_any_mouse_down(cx.listener(|root, _: &MouseDownEvent, _window, cx| {
                    root.close_remind_picker();
                    cx.stop_propagation();
                    cx.notify();
                }))
            })
            .child(card),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> u64 {
        reminder::at_local(y, m, d, h, min)
    }

    #[test]
    fn it_opens_on_tomorrow_at_nine_never_repeating_or_on_the_reminder_there_is() {
        let now = at(2026, 11, 30, 15, 0);
        let p = Picker::new(7, now, None);
        assert_eq!((p.day, p.minutes, p.repeat, (p.year, p.month)), ((2026, 12, 1), 540, Repeat::Never, (2026, 12)));
        let r = reminder::Reminder::new(at(2026, 12, 4, 14, 30), Repeat::Weekly);
        let p = Picker::new(7, now, Some(&r));
        assert_eq!((p.day, p.minutes, p.repeat), ((2026, 12, 4), 14 * 60 + 30, Repeat::Weekly));
    }

    #[test]
    fn a_month_is_laid_out_in_weeks_from_sunday() {
        let mut p = Picker::new(7, at(2026, 11, 16, 10, 0), None);
        p.year = 2026;
        p.month = 11;
        // November 2026 begins on a Sunday and has 30 days: five weeks.
        let weeks = p.weeks();
        assert_eq!(weeks.len(), 5);
        assert_eq!(weeks[0], [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6), Some(7)]);
        assert_eq!(weeks[4], [Some(29), Some(30), None, None, None, None, None]);
        p.turn_month(1);
        // December 2026 begins on a Tuesday.
        assert_eq!(p.weeks()[0], [None, None, Some(1), Some(2), Some(3), Some(4), Some(5)]);
        p.turn_month(1);
        assert_eq!((p.year, p.month), (2027, 1), "across a year's end");
        p.turn_month(-13);
        assert_eq!((p.year, p.month), (2025, 12));
    }

    #[test]
    fn a_day_before_today_cannot_be_chosen_and_the_arrows_move_through_months() {
        let now = at(2026, 11, 16, 10, 0);
        let mut p = Picker::new(7, now, None);
        p.choose(10, now);
        assert_eq!(p.day, (2026, 11, 17), "the 10th is past");
        p.choose(16, now);
        assert_eq!(p.day, (2026, 11, 16), "today may be chosen");
        assert!(p.is_today((2026, 11, 16), now));
        p.move_day(-1, now);
        assert_eq!(p.day, (2026, 11, 16), "not before today");
        p.move_day(15, now);
        assert_eq!((p.day, p.month), ((2026, 12, 1), 12), "into the next month, which is shown");
    }

    #[test]
    fn times_come_every_fifteen_minutes_and_a_typed_time_is_read_in_either_clock() {
        assert_eq!(times().len(), 96);
        assert_eq!((time_label(0), time_label(9 * 60), time_label(12 * 60 + 30), time_label(23 * 60 + 45)), ("12:00 AM".into(), "9:00 AM".into(), "12:30 PM".into(), "11:45 PM".into()));
        for (text, m) in [("14:30", 870), ("2:30pm", 870), ("2:30 PM", 870), ("2pm", 840), ("9:15am", 555), ("12am", 0), ("12pm", 720), ("0:05", 5), ("9:15 a.m.", 555)] {
            assert_eq!(parse_time(text), Some(m), "{text}");
        }
        for text in ["", "25:00", "13pm", "9:7", "9:60", "noon", "9"] {
            assert_eq!(parse_time(text), None, "{text}");
        }
        let mut p = Picker::new(7, at(2026, 11, 16, 10, 0), None);
        assert!(p.type_time("2pm"));
        assert_eq!(p.minutes, 840);
        assert!(!p.type_time("later"));
        assert!(p.time_bad);
        assert_eq!(p.minutes, 840, "a time that cannot be read changes nothing");
    }

    #[test]
    fn save_is_for_a_time_still_to_come_and_sets_the_time_and_repeat_chosen() {
        let now = at(2026, 11, 16, 10, 0);
        let mut p = Picker::new(7, now, None);
        p.choose(16, now);
        p.minutes = 9 * 60;
        assert!(!p.can_save(now), "this morning has gone");
        p.minutes = 10 * 60 + 15;
        assert!(p.can_save(now));
        p.repeat = Repeat::Weekdays;
        assert_eq!(p.reminder(), (at(2026, 11, 16, 10, 15), Repeat::Weekdays));
    }

    #[test]
    fn the_summary_and_the_repeats_are_said_for_the_day_chosen() {
        let now = at(2026, 11, 16, 10, 0);
        let mut p = Picker::new(7, now, None);
        assert_eq!(p.summary(now), "Tomorrow at 9:00 AM");
        p.choose(20, now);
        p.minutes = 14 * 60 + 30;
        p.repeat = Repeat::Weekly;
        assert_eq!(p.summary(now), "Fri, Nov 20 at 2:30 PM, every week on Friday");
        let names: Vec<String> = p.repeats().into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["Never", "Every day", "Every weekday", "Every week on Friday", "Every month on the 20th"]);
        p.choose(16, now);
        p.repeat = Repeat::Never;
        assert_eq!(p.summary(now), "Today at 2:30 PM");
        p.turn_month(2);
        p.choose(5, now);
        assert_eq!(p.summary(now), "Tue, Jan 5, 2027 at 2:30 PM");
    }
}
