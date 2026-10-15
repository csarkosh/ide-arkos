//! The docking engine (spec 8): pure over a per-tick snapshot of Chrome's windows and the
//! browser tabs' placeholders, it decides what to dock, move, hide, show, raise or undock; the
//! window applies the actions through `Ax`. Nothing here calls the Accessibility API.

use crate::ax::{AxWindow, Rect};
use crate::tree::TabId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long after arkos launched a window for a tab that tab docks the next new window even
/// with no session working.
pub const LAUNCH_GRACE: Duration = Duration::from_secs(10);
/// The tick while every browser tab waits for a window (or for the permission): 1 Hz.
pub const WAIT_POLL: Duration = Duration::from_secs(1);
/// The tick while a window is docked, or the mouse is down with a browser tab present: 20 Hz.
pub const TRACK_POLL: Duration = Duration::from_millis(50);
/// A frame within this many points of the target is in place: AX rounds, so an exact compare
/// would write every tick.
const IN_PLACE: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub enum DockState {
    /// Accessibility access is missing: the tab shows how to grant it.
    NoPermission,
    /// No window yet. `launched_at`: arkos launched a window for this tab then.
    Waiting { since: Instant, launched_at: Option<Instant> },
    /// `shown`: the rectangle last written while visible; `parked`: where the window was when it
    /// was hidden, `None` while visible.
    Docked { win: AxWindow, shown: Option<Rect>, parked: Option<Rect> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Dock { tab: TabId, win: AxWindow },
    Move { win: AxWindow, rect: Rect },
    Hide { win: AxWindow, to: Rect },
    Show { win: AxWindow, rect: Rect },
    Raise(AxWindow),
    /// `win` is `None` when the window is gone; `leave_at` is where to put it back, if anywhere.
    Undock { tab: TabId, win: Option<AxWindow>, leave_at: Option<Rect> },
    Title { tab: TabId, title: String },
    Permission(bool),
}

/// What the window reads once per tick, on the UI thread.
pub struct Snapshot {
    pub trusted: bool,
    /// Chrome's windows with their frames and whether each is minimized.
    pub windows: Vec<(AxWindow, Option<Rect>, bool)>,
    pub titles: Vec<(AxWindow, String)>,
    /// Browser tabs in reading order with their placeholder's global rect, `None` when the
    /// tab is not the active one in its pane (or its pane is not shown).
    pub placeholders: Vec<(TabId, Option<Rect>)>,
    pub any_working: bool,
    pub mouse_down: bool,
    pub window_active: bool,
    /// The arkos window is minimized or hidden.
    pub window_hidden: bool,
    /// The right edge of the union of every display: hidden windows park past it.
    pub screens_right: f64,
    pub now: Instant,
}

/// Whether a docked, visible window must be written: its placeholder moved since the last write,
/// or (the mouse up) the window left the frame it settled at after that write. `settled` is
/// `None` on the tick right after a write: whatever frame the window landed at then is taken as
/// settled, so a window Chrome clamps to its minimum size is not written again every tick.
fn needs_move(frame: Option<Rect>, shown: Option<Rect>, settled: Option<Rect>, target: Rect, mouse_down: bool) -> bool {
    let near = |a: Rect, b: Rect| a.distance_outside(&b) < IN_PLACE;
    if shown.is_none_or(|s| !near(s, target)) {
        return true;
    }
    let Some(frame) = frame else { return false };
    !mouse_down && !near(frame, target) && settled.is_some_and(|s| !near(s, frame))
}

/// `/usr/bin/open`'s arguments for a new Chrome window in the user's profile: a normal window
/// (its tab strip kept, spec 8) or, for a single page, app mode. No URL: a blank window.
pub fn chrome_open_args(url: Option<&str>, app_mode: bool) -> Vec<String> {
    let mut args: Vec<String> = ["-na", "Google Chrome", "--args"].iter().map(|s| s.to_string()).collect();
    match (url, app_mode) {
        (Some(u), true) => args.push(format!("--app={u}")),
        (Some(u), false) => args.extend(["--new-window".to_string(), u.to_string()]),
        (None, _) => args.push("--new-window".to_string()),
    }
    args
}

#[derive(Default)]
pub struct Docks {
    pub tabs: BTreeMap<TabId, DockState>,
    /// Last tick's Chrome windows: a window not in it is new.
    known: Vec<AxWindow>,
    /// `known` was filled once since tracking started: on the first tick every window is old.
    primed: bool,
    /// The mouse was down on the last tick: a window appearing on the tick it comes up is a
    /// drag's too.
    last_mouse_down: bool,
    /// The arkos window was active on the last tick (`None` before the first).
    was_active: Option<bool>,
    /// The permission as last seen; `None` before the first tick (taken as granted).
    trusted: Option<bool>,
    /// Where each docked window was last shown, kept after undocking so the caller can put a
    /// hidden window back there.
    last_shown: BTreeMap<TabId, Rect>,
    /// The frame a window settled at on the tick after a write (Chrome may clamp it).
    settled: BTreeMap<TabId, Rect>,
    /// Each docked tab's title as last reported.
    titles: BTreeMap<TabId, String>,
}

impl Docks {
    /// A new browser tab, waiting; `launched`: arkos just launched a window for it.
    pub fn add(&mut self, tab: TabId, launched: bool, now: Instant) {
        if self.tabs.is_empty() {
            // Tracking (re)starts: the windows already open are old.
            self.primed = false;
        }
        let state = if self.trusted == Some(false) { DockState::NoPermission } else { DockState::Waiting { since: now, launched_at: launched.then_some(now) } };
        self.tabs.insert(tab, state);
    }

    /// The tab is gone: forget it, returning its window to release.
    pub fn remove(&mut self, tab: TabId) -> Option<AxWindow> {
        self.last_shown.remove(&tab);
        self.settled.remove(&tab);
        self.titles.remove(&tab);
        match self.tabs.remove(&tab)? {
            DockState::Docked { win, .. } => Some(win),
            _ => None,
        }
    }

    /// Docks `win` into `tab` (the palette's choice), taking it from any other tab holding it.
    pub fn dock_by_choice(&mut self, tab: TabId, win: AxWindow) {
        let holders: Vec<TabId> = self.tabs.iter().filter(|(t, st)| **t != tab && matches!(st, DockState::Docked { win: w, .. } if *w == win)).map(|(t, _)| *t).collect();
        for t in holders {
            self.undock(t);
        }
        if !self.known.contains(&win) {
            self.known.push(win.clone());
        }
        self.settled.remove(&tab);
        self.titles.remove(&tab);
        self.tabs.insert(tab, DockState::Docked { win, shown: None, parked: None });
    }

    /// Releases the tab's window; the tab stays, waiting. `last_shown` still answers for it.
    pub fn undock(&mut self, tab: TabId) -> Option<AxWindow> {
        let state = self.tabs.get_mut(&tab)?;
        let DockState::Docked { .. } = state else { return None };
        let DockState::Docked { win, .. } = std::mem::replace(state, DockState::Waiting { since: Instant::now(), launched_at: None }) else { unreachable!() };
        self.settled.remove(&tab);
        self.titles.remove(&tab);
        Some(win)
    }

    pub fn docked_window(&self, tab: TabId) -> Option<&AxWindow> {
        match self.tabs.get(&tab)? {
            DockState::Docked { win, .. } => Some(win),
            _ => None,
        }
    }

    pub fn waiting_tabs(&self) -> Vec<TabId> {
        self.tabs.iter().filter(|(_, st)| matches!(st, DockState::Waiting { .. })).map(|(t, _)| *t).collect()
    }

    pub fn last_shown(&self, tab: TabId) -> Option<Rect> {
        self.last_shown.get(&tab).copied()
    }

    /// The permission as the last tick saw it (granted before the first tick).
    pub fn trusted(&self) -> bool {
        self.trusted != Some(false)
    }

    /// An AX write failed as not trusted: the next tick re-reads the permission and, when it is
    /// gone, puts every tab on the permission screen.
    pub fn distrust(&mut self) {
        self.trusted = None;
    }

    /// How long until the next tick: `None` with no browser tab (the timer stops).
    pub fn cadence(&self, mouse_down: bool) -> Option<Duration> {
        if self.tabs.is_empty() {
            None
        } else if mouse_down || self.tabs.values().any(|st| matches!(st, DockState::Docked { .. })) {
            Some(TRACK_POLL)
        } else {
            Some(WAIT_POLL)
        }
    }

    pub fn tick(&mut self, s: &Snapshot) -> Vec<Action> {
        let mut out = Vec::new();
        let current: Vec<AxWindow> = s.windows.iter().map(|(w, _, _)| w.clone()).collect();
        let was_active = self.was_active.replace(s.window_active);
        let mouse_was_down = std::mem::replace(&mut self.last_mouse_down, s.mouse_down);
        // Permission first: a change flips every tab between NoPermission and Waiting.
        if self.trusted.unwrap_or(true) != s.trusted {
            self.trusted = Some(s.trusted);
            for state in self.tabs.values_mut() {
                *state = if s.trusted { DockState::Waiting { since: s.now, launched_at: None } } else { DockState::NoPermission };
            }
            self.settled.clear();
            self.titles.clear();
            self.known = current;
            self.primed = s.trusted;
            out.push(Action::Permission(s.trusted));
            return out;
        }
        self.trusted = Some(s.trusted);
        if !s.trusted {
            return out;
        }
        if !self.primed {
            self.known = current.clone();
            self.primed = true;
        }
        let new: Vec<AxWindow> = current.iter().filter(|w| !self.known.contains(w)).cloned().collect();
        // A docked window that is gone: back to waiting, the URL kept by the tab.
        for (tab, state) in self.tabs.iter_mut() {
            if let DockState::Docked { win, .. } = state {
                if !current.contains(win) {
                    out.push(Action::Undock { tab: *tab, win: None, leave_at: None });
                    *state = DockState::Waiting { since: s.now, launched_at: None };
                    self.settled.remove(tab);
                    self.titles.remove(tab);
                }
            }
        }
        // Auto-dock: one new window per tick into the first waiting tab whose placeholder shows,
        // in reading order, when a session works or that tab launched a window within the grace;
        // never while the mouse is down (a Chrome tab dragged out makes a window of its own).
        if !s.mouse_down && !mouse_was_down {
            if let Some(win) = new.first() {
                let target = s.placeholders.iter().find(|(tab, rect)| {
                    rect.is_some()
                        && match self.tabs.get(tab) {
                            Some(DockState::Waiting { launched_at, .. }) => s.any_working || launched_at.is_some_and(|t| s.now.saturating_duration_since(t) <= LAUNCH_GRACE),
                            _ => false,
                        }
                });
                if let Some((tab, _)) = target {
                    self.tabs.insert(*tab, DockState::Docked { win: win.clone(), shown: None, parked: None });
                    out.push(Action::Dock { tab: *tab, win: win.clone() });
                }
            }
        }
        // Tracking, hiding and showing, titles.
        let placeholder_of = |tab: TabId| s.placeholders.iter().find(|(t, _)| *t == tab).and_then(|(_, r)| *r);
        for (tab, state) in self.tabs.iter_mut() {
            let DockState::Docked { win, shown, parked } = state else { continue };
            let frame = s.windows.iter().find(|(w, _, _)| w == win).and_then(|(_, f, _)| *f);
            match placeholder_of(*tab).filter(|_| !s.window_hidden) {
                Some(target) => {
                    if parked.is_some() {
                        out.push(Action::Show { win: win.clone(), rect: target });
                        *parked = None;
                        *shown = Some(target);
                        self.settled.remove(tab);
                        self.last_shown.insert(*tab, target);
                    } else if needs_move(frame, *shown, self.settled.get(tab).copied(), target, s.mouse_down) {
                        out.push(Action::Move { win: win.clone(), rect: target });
                        *shown = Some(target);
                        self.settled.remove(tab);
                        self.last_shown.insert(*tab, target);
                    } else if let Some(f) = frame {
                        self.settled.entry(*tab).or_insert(f);
                    }
                    if was_active == Some(false) && s.window_active && parked.is_none() {
                        out.push(Action::Raise(win.clone()));
                    }
                }
                None => {
                    if parked.is_none() {
                        let from = frame.or(*shown).unwrap_or(Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 });
                        out.push(Action::Hide { win: win.clone(), to: from.offscreen(s.screens_right) });
                        *parked = Some(from);
                    }
                }
            }
            if let Some((_, title)) = s.titles.iter().find(|(w, _)| w == win) {
                if self.titles.get(tab) != Some(title) {
                    self.titles.insert(*tab, title.clone());
                    out.push(Action::Title { tab: *tab, title: title.clone() });
                }
            }
        }
        self.known = current;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ax::{Ax, FakeAx, OFFSCREEN_GAP};

    fn snap(ax: &FakeAx, placeholders: Vec<(TabId, Option<Rect>)>, now: Instant) -> Snapshot {
        let pid = ax.chrome_pid().unwrap();
        let windows = ax.windows(pid);
        Snapshot {
            trusted: ax.trusted(false),
            titles: windows.iter().filter_map(|w| Some((w.clone(), ax.title(w)?))).collect(),
            windows: windows
                .into_iter()
                .map(|w| {
                    let f = ax.frame(&w);
                    let m = ax.minimized(&w);
                    (w, f, m)
                })
                .collect(),
            placeholders,
            any_working: false,
            mouse_down: ax.mouse_down(),
            window_active: true,
            window_hidden: false,
            screens_right: 1512.0,
            now,
        }
    }
    const R: Rect = Rect { x: 300.0, y: 200.0, w: 800.0, h: 600.0 };
    const R2: Rect = Rect { x: 1200.0, y: 200.0, w: 500.0, h: 600.0 };
    const TINY: Rect = Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };

    #[test]
    fn a_waiting_tab_docks_the_first_new_window_while_a_session_works() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let _old = ax.add_window("Old", TINY);
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.any_working = true;
        assert_eq!(d.tick(&s), vec![], "a window that was already open is not docked");
        let new = ax.add_window("Claude", TINY);
        let mut s = snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(1));
        s.any_working = true;
        let actions = d.tick(&s);
        assert_eq!(actions[0], Action::Dock { tab: 1, win: new.clone() });
        assert!(actions.contains(&Action::Move { win: new.clone(), rect: R }));
        assert!(actions.contains(&Action::Title { tab: 1, title: "Claude".into() }));
        assert_eq!(d.docked_window(1), Some(&new));
        // Not working: a further new window is ignored.
        let _other = ax.add_window("Other", TINY);
        d.add(2, false, t0);
        let s = snap(&ax, vec![(1, Some(R)), (2, Some(R2))], t0 + Duration::from_secs(2));
        assert!(!d.tick(&s).iter().any(|a| matches!(a, Action::Dock { .. })), "no working session: nothing auto-docks");
    }

    #[test]
    fn one_new_window_docks_into_the_first_waiting_tab_only() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(5, false, t0);
        d.add(2, false, t0);
        let mut s = snap(&ax, vec![(2, Some(R)), (5, Some(R2))], t0);
        s.any_working = true;
        d.tick(&s);
        let w = ax.add_window("New", TINY);
        let mut s = snap(&ax, vec![(2, Some(R)), (5, Some(R2))], t0 + Duration::from_secs(1));
        s.any_working = true;
        let docks: Vec<_> = d.tick(&s).into_iter().filter(|a| matches!(a, Action::Dock { .. })).collect();
        assert_eq!(docks, vec![Action::Dock { tab: 2, win: w }], "reading order: the placeholders' order, not the tab ids");
        assert!(d.waiting_tabs().contains(&5));
    }

    #[test]
    fn two_new_windows_dock_one_per_tick() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        d.add(2, false, t0);
        let places = || vec![(1, Some(R)), (2, Some(R2))];
        let mut s = snap(&ax, places(), t0);
        s.any_working = true;
        d.tick(&s);
        let a = ax.add_window("A", TINY);
        let _b = ax.add_window("B", TINY);
        let mut s = snap(&ax, places(), t0);
        s.any_working = true;
        let docks: Vec<_> = d.tick(&s).into_iter().filter(|a| matches!(a, Action::Dock { .. })).collect();
        assert_eq!(docks, vec![Action::Dock { tab: 1, win: a }]);
        let mut s = snap(&ax, places(), t0);
        s.any_working = true;
        assert!(!d.tick(&s).iter().any(|a| matches!(a, Action::Dock { .. })), "the second one is known now, not new");
    }

    #[test]
    fn windows_that_appear_while_the_mouse_is_down_are_not_auto_docked() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.any_working = true;
        d.tick(&s);
        ax.set_mouse_down(true);
        let _w = ax.add_window("Dragged out", TINY);
        let mut s = snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(1));
        s.any_working = true;
        assert!(d.tick(&s).is_empty());
        ax.set_mouse_down(false);
        let mut s = snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(2));
        s.any_working = true;
        assert!(d.tick(&s).is_empty(), "it is a known window now, never a new one");
    }

    #[test]
    fn a_window_appearing_on_the_tick_the_mouse_comes_up_is_not_auto_docked() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        ax.set_mouse_down(true);
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.any_working = true;
        d.tick(&s);
        ax.set_mouse_down(false);
        let _w = ax.add_window("Dropped", TINY);
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.any_working = true;
        assert!(d.tick(&s).is_empty());
    }

    #[test]
    fn a_launched_tab_docks_the_next_window_without_a_working_session_within_the_grace() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, true, t0);
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        let w = ax.add_window("Example", TINY);
        let actions = d.tick(&snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(3)));
        assert_eq!(actions[0], Action::Dock { tab: 1, win: w });
        let mut d2 = Docks::default();
        d2.add(9, true, t0);
        d2.tick(&snap(&ax, vec![(9, Some(R))], t0));
        let _late = ax.add_window("Late", TINY);
        assert!(d2.tick(&snap(&ax, vec![(9, Some(R))], t0 + Duration::from_secs(11))).is_empty(), "past the grace it waits like any tab");
    }

    #[test]
    fn a_moved_placeholder_moves_the_window_and_hiding_parks_it_off_screen() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        assert!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)).contains(&Action::Move { win: w.clone(), rect: R }));
        ax.set_frame(&w, R).unwrap();
        assert!(!d.tick(&snap(&ax, vec![(1, Some(R))], t0)).iter().any(|a| matches!(a, Action::Move { .. })), "in place: no write");
        let moved = Rect { x: 350.0, ..R };
        assert!(d.tick(&snap(&ax, vec![(1, Some(moved))], t0)).contains(&Action::Move { win: w.clone(), rect: moved }));
        // The tab goes inactive: hide (park off screen); it comes back: show at the placeholder.
        let mut s = snap(&ax, vec![(1, None)], t0);
        s.screens_right = 1512.0;
        let hide = d.tick(&s);
        assert_eq!(hide, vec![Action::Hide { win: w.clone(), to: Rect { x: 1512.0 + OFFSCREEN_GAP, y: 200.0, w: 800.0, h: 600.0 } }]);
        assert!(d.tick(&s).is_empty(), "hidden once");
        let show = d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        assert_eq!(show, vec![Action::Show { win: w.clone(), rect: R }]);
        // The arkos window minimized hides it too.
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.window_hidden = true;
        s.screens_right = 1512.0;
        assert!(matches!(d.tick(&s)[0], Action::Hide { .. }));
    }

    #[test]
    fn hidden_windows_park_right_of_every_display() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        let mut s = snap(&ax, vec![(1, None)], t0);
        s.screens_right = 1512.0 + 2560.0;
        assert_eq!(d.tick(&s), vec![Action::Hide { win: w, to: Rect { x: 4072.0 + OFFSCREEN_GAP, ..R } }]);
    }

    #[test]
    fn a_window_the_user_moves_snaps_back_once_the_mouse_is_up() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        ax.set_frame_outside(&w, Rect { x: 320.0, ..R });
        ax.set_mouse_down(true);
        assert!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)).is_empty(), "not while the user drags it");
        ax.set_mouse_down(false);
        assert_eq!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)), vec![Action::Move { win: w, rect: R }]);
    }

    #[test]
    fn a_window_chrome_clamps_is_not_written_every_tick() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        let small = Rect { w: 200.0, ..R };
        assert!(d.tick(&snap(&ax, vec![(1, Some(small))], t0)).contains(&Action::Move { win: w.clone(), rect: small }));
        // Chrome keeps its minimum width: the frame stays R.
        assert!(d.tick(&snap(&ax, vec![(1, Some(small))], t0)).is_empty());
        assert!(d.tick(&snap(&ax, vec![(1, Some(small))], t0)).is_empty());
    }

    #[test]
    fn titles_are_reported_when_they_change_only() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("One", R);
        d.dock_by_choice(1, w.clone());
        assert!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)).contains(&Action::Title { tab: 1, title: "One".into() }));
        assert!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)).is_empty());
        ax.set_title(&w, "Two");
        assert_eq!(d.tick(&snap(&ax, vec![(1, Some(R))], t0)), vec![Action::Title { tab: 1, title: "Two".into() }]);
    }

    #[test]
    fn activating_the_arkos_window_raises_the_visible_docked_windows() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        d.add(2, false, t0);
        let a = ax.add_window("A", R);
        let b = ax.add_window("B", R2);
        d.dock_by_choice(1, a.clone());
        d.dock_by_choice(2, b);
        d.tick(&snap(&ax, vec![(1, Some(R)), (2, None)], t0));
        let mut s = snap(&ax, vec![(1, Some(R)), (2, None)], t0);
        s.window_active = false;
        assert!(!d.tick(&s).iter().any(|a| matches!(a, Action::Raise(_))));
        let raised: Vec<_> = d.tick(&snap(&ax, vec![(1, Some(R)), (2, None)], t0)).into_iter().filter(|a| matches!(a, Action::Raise(_))).collect();
        assert_eq!(raised, vec![Action::Raise(a)], "the hidden one stays parked");
    }

    #[test]
    fn a_docked_window_that_vanishes_leaves_the_tab_waiting() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        ax.remove(&w);
        let actions = d.tick(&snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(1)));
        assert_eq!(actions, vec![Action::Undock { tab: 1, win: None, leave_at: None }]);
        assert!(d.waiting_tabs().contains(&1));
    }

    #[test]
    fn a_failing_ax_call_puts_the_tab_back_on_the_permission_screen() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        ax.set_trusted(false);
        let mut s = snap(&ax, vec![(1, Some(R))], t0);
        s.trusted = false;
        let actions = d.tick(&s);
        assert_eq!(actions, vec![Action::Permission(false)]);
        assert!(matches!(d.tabs[&1], DockState::NoPermission));
        assert_eq!(d.cadence(false), Some(WAIT_POLL), "it keeps checking for the grant, slowly");
        d.add(2, false, t0);
        assert!(matches!(d.tabs[&2], DockState::NoPermission), "a tab added meanwhile waits for the grant too");
        ax.set_trusted(true);
        let mut s = snap(&ax, vec![(1, Some(R))], t0 + Duration::from_secs(1));
        s.trusted = true;
        assert_eq!(d.tick(&s), vec![Action::Permission(true)]);
        assert!(matches!(d.tabs[&1], DockState::Waiting { .. }));
    }

    #[test]
    fn the_cadence_is_slow_while_waiting_fast_while_tracking_and_off_otherwise() {
        let t0 = Instant::now();
        let mut d = Docks::default();
        assert_eq!(d.cadence(false), None);
        d.add(1, false, t0);
        assert_eq!(d.cadence(false), Some(WAIT_POLL));
        assert_eq!(d.cadence(true), Some(TRACK_POLL), "mouse down with a browser tab: drag detection");
        let ax = FakeAx::new(true);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w);
        assert_eq!(d.cadence(false), Some(TRACK_POLL));
        d.remove(1);
        assert_eq!(d.cadence(false), None);
    }

    #[test]
    fn closing_a_tab_undocks_and_leaves_the_window_where_it_was_shown() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        d.tick(&snap(&ax, vec![(1, Some(R))], t0));
        let mut s = snap(&ax, vec![(1, None)], t0);
        s.screens_right = 1512.0;
        d.tick(&s); // hidden now
        assert_eq!(d.undock(1), Some(w.clone()));
        // The caller moves a hidden window back to where it was shown before the hide.
        assert_eq!(d.last_shown(1), Some(R));
        assert!(d.waiting_tabs().contains(&1), "undocked, the tab stays and waits");
        assert_eq!(d.remove(1), None, "nothing docked any more");
    }

    #[test]
    fn chrome_opens_a_normal_window_or_a_single_page() {
        assert_eq!(chrome_open_args(Some("https://example.com"), false), ["-na", "Google Chrome", "--args", "--new-window", "https://example.com"]);
        assert_eq!(chrome_open_args(Some("https://example.com"), true), ["-na", "Google Chrome", "--args", "--app=https://example.com"]);
        assert_eq!(chrome_open_args(None, true), ["-na", "Google Chrome", "--args", "--new-window"], "no page for app mode: a blank window");
    }

    #[test]
    fn docking_a_window_another_tab_holds_moves_it() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(1, false, t0);
        d.add(2, false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(1, w.clone());
        d.dock_by_choice(2, w.clone());
        assert_eq!(d.docked_window(2), Some(&w));
        assert_eq!(d.docked_window(1), None);
    }
}
