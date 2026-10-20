//! The video tab's playback core, headless (spec 3 and 4): the time forms the strip and the CLI
//! speak, the rule that decides when a tab follows its file, the `Player` trait the AVFoundation
//! player implements, and `VideoModel`, the state machine the view drives. Nothing here touches
//! gpui or AVFoundation, so every line is tested with a `FakePlayer`.

// Nothing outside the tests calls this until the video view (Task 6) is written.
#![allow(dead_code)]

use protocol::{LoopChange, VideoState};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a notice ("new cut", "loop cleared") stays on the strip.
pub const NOTICE_FOR: Duration = Duration::from_secs(3);

/// `config_watch::fingerprint`'s shape: modification time in nanoseconds and size, None when the
/// file is missing.
pub type Fp = Option<(u128, u64)>;

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `12`, `12.5`, `m:ss`, `m:ss.fff` and `h:mm:ss[.fff]` to seconds; Err(text) otherwise.
pub fn parse_time(s: &str) -> Result<f64, String> {
    let bad = || format!("not a time: {s:?}");
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() > 3 {
        return Err(bad());
    }
    let last = parts[parts.len() - 1];
    let (whole, frac) = match last.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (last, None),
    };
    if !digits(whole) || frac.is_some_and(|f| !digits(f)) {
        return Err(bad());
    }
    let secs: f64 = last.parse().map_err(|_| bad())?;
    let mut total = secs;
    if parts.len() > 1 {
        if secs >= 60.0 {
            return Err(bad());
        }
        let mut scale = 60.0;
        for (i, p) in parts[..parts.len() - 1].iter().rev().enumerate() {
            if !digits(p) {
                return Err(bad());
            }
            let n: f64 = p.parse().map_err(|_| bad())?;
            // Minutes under an hour when there is an hours field; the leading field is free.
            if parts.len() == 3 && i == 0 && n >= 60.0 {
                return Err(bad());
            }
            total += n * scale;
            scale *= 60.0;
        }
    }
    Ok(total)
}

/// `m:ss.f`, with hours when 3600 or more: 72.25 is "1:12.2", 3725.0 is "1:02:05.0".
pub fn fmt_time(secs: f64) -> String {
    let secs = if secs.is_finite() && secs > 0.0 { secs } else { 0.0 };
    let tenths = (secs * 10.0 + 1e-6).floor() as u64;
    let (t, whole) = (tenths % 10, tenths / 10);
    let (h, m, s) = (whole / 3600, whole / 60 % 60, whole % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{t}")
    } else {
        format!("{m}:{s:02}.{t}")
    }
}

/// `a-b` with either side a time and a before b.
pub fn parse_range(s: &str) -> Result<(f64, f64), String> {
    let (a, b) = s.split_once('-').ok_or_else(|| format!("not a range: {s:?}"))?;
    let (a, b) = (parse_time(a)?, parse_time(b)?);
    if a < b {
        Ok((a, b))
    } else {
        Err(format!("the range must start before it ends: {s:?}"))
    }
}

/// A duration the player reported, when it is a real length.
fn known(d: Option<f64>) -> Option<f64> {
    d.filter(|d| d.is_finite() && *d >= 0.0)
}

/// What a poll of the file decides (spec 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    Nothing,
    Reload,
    Waiting,
    Back,
}

/// Spec 4: `shown` is the fingerprint of the file as loaded, `prev` one poll ago, `now` this poll.
/// Nothing while the file is changing, a reload once it has been stable for a poll and differs
/// from the one shown, waiting when it is gone, and back from waiting on the first stable file.
pub fn follow(shown: Fp, prev: Fp, now: Fp, waiting: bool) -> Follow {
    if now.is_none() {
        return Follow::Waiting;
    }
    if prev != now {
        return Follow::Nothing;
    }
    if waiting {
        Follow::Back
    } else if now != shown {
        Follow::Reload
    } else {
        Follow::Nothing
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    Loading,
    Ready,
    Reloading,
    Waiting,
    Unplayable(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    Ready { duration: f64, width: u32, height: u32 },
    Failed(String),
    Ended,
}

/// What the model needs from a player; AVFoundation implements it (Task 5).
pub trait Player {
    /// Starts loading a new item, for the same path or another. The old item keeps showing until
    /// the new one is `Ready`.
    fn load(&mut self, path: &Path);
    fn play(&mut self);
    fn pause(&mut self);
    fn seek(&mut self, secs: f64);
    fn step(&mut self, frames: i32);
    fn set_muted(&mut self, muted: bool);
    fn set_loop(&mut self, range: Option<(f64, f64)>);
    fn position(&self) -> f64;
    fn duration(&self) -> Option<f64>;
    fn size(&self) -> Option<(u32, u32)>;
    /// What happened since the last poll.
    fn poll(&mut self) -> Vec<PlayerEvent>;
    /// The newest frame for the surface element to paint, when there is one.
    #[cfg(target_os = "macos")]
    fn frame(&mut self) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
        None
    }
}

/// A player for tests: records its calls as strings and hands back `pending` from `poll`.
#[cfg(test)]
#[derive(Default)]
pub struct FakePlayer {
    pub calls: Vec<String>,
    pub pending: Vec<PlayerEvent>,
    pub pos: f64,
    pub dur: Option<f64>,
    pub size: Option<(u32, u32)>,
}

#[cfg(test)]
impl Player for FakePlayer {
    fn load(&mut self, path: &Path) {
        self.calls.push(format!("load {}", path.display()));
    }
    fn play(&mut self) {
        self.calls.push("play".into());
    }
    fn pause(&mut self) {
        self.calls.push("pause".into());
    }
    fn seek(&mut self, secs: f64) {
        self.calls.push(format!("seek {secs:?}"));
        self.pos = secs;
    }
    fn step(&mut self, frames: i32) {
        self.calls.push(format!("step {frames}"));
    }
    fn set_muted(&mut self, muted: bool) {
        self.calls.push(format!("muted {muted}"));
    }
    fn set_loop(&mut self, range: Option<(f64, f64)>) {
        self.calls.push(format!("loop {range:?}"));
    }
    fn position(&self) -> f64 {
        self.pos
    }
    fn duration(&self) -> Option<f64> {
        self.dur
    }
    fn size(&self) -> Option<(u32, u32)> {
        self.size
    }
    fn poll(&mut self) -> Vec<PlayerEvent> {
        let events = std::mem::take(&mut self.pending);
        for e in &events {
            if let PlayerEvent::Ready { duration, width, height } = e {
                self.dur = Some(*duration);
                self.size = Some((*width, *height));
            }
        }
        events
    }
}

/// One video tab's state: what the user asked for, what the player is doing, and the file it
/// follows.
pub struct VideoModel {
    pub path: PathBuf,
    pub state: PlayerState,
    /// The player is running now.
    pub playing: bool,
    /// What the user last asked for; survives a pause for switching away or a reload.
    pub wanted_playing: bool,
    pub muted: bool,
    pub loop_range: Option<(f64, f64)>,
    /// The fingerprint of the file as loaded.
    pub shown_fp: Fp,
    /// The fingerprint one poll ago.
    pub prev_fp: Fp,
    /// A short message for the strip and when it was set (`NOTICE_FOR`).
    pub notice: Option<(String, Instant)>,
    /// The tab is not the active one of its pane, or the window is hidden.
    pub inactive: bool,
    /// Where to seek once the item being loaded is ready.
    restore_at: Option<f64>,
    /// An item has been ready at least once, so a reload keeps showing it.
    loaded: bool,
}

impl VideoModel {
    pub fn new(path: PathBuf, at: Option<f64>, loop_range: Option<(f64, f64)>) -> Self {
        VideoModel {
            path,
            state: PlayerState::Loading,
            playing: false,
            wanted_playing: false,
            muted: false,
            loop_range,
            shown_fp: None,
            prev_fp: None,
            notice: None,
            inactive: false,
            restore_at: at,
            loaded: false,
        }
    }

    /// Starts loading, or waits when the file is not there yet.
    pub fn open(&mut self, p: &mut dyn Player, fp: Fp) {
        self.shown_fp = fp;
        self.prev_fp = fp;
        if fp.is_some() {
            self.reload_now(p);
        } else {
            self.state = PlayerState::Waiting;
        }
    }

    /// One poll of the file: decides by `follow`, then applies what the player reports. True when
    /// a reload started.
    pub fn tick(&mut self, p: &mut dyn Player, fp: Fp, now: Instant) -> bool {
        let waiting = self.state == PlayerState::Waiting;
        let decision = follow(self.shown_fp, self.prev_fp, fp, waiting);
        self.prev_fp = fp;
        let mut started = false;
        match decision {
            Follow::Reload | Follow::Back => {
                self.shown_fp = fp;
                self.reload_now(p);
                started = true;
            }
            Follow::Waiting => {
                if !waiting {
                    self.pause_player(p);
                    self.state = PlayerState::Waiting;
                }
            }
            Follow::Nothing => {}
        }
        self.apply(p, now);
        started
    }

    /// Loads the file again at once, keeping the position, the rate and the loop.
    pub fn reload_now(&mut self, p: &mut dyn Player) {
        if self.loaded {
            self.restore_at = Some(p.position());
            self.state = PlayerState::Reloading;
        } else {
            self.state = PlayerState::Loading;
        }
        // A reload asked for by hand loads the file as it is now, so the next poll finds nothing new.
        self.shown_fp = self.prev_fp;
        p.load(&self.path);
    }

    /// Takes what the player reports since the last call.
    pub fn apply(&mut self, p: &mut dyn Player, now: Instant) {
        for event in p.poll() {
            match event {
                PlayerEvent::Ready { duration, .. } => self.ready(p, duration, now),
                PlayerEvent::Failed(why) => match self.state {
                    PlayerState::Loading => self.state = PlayerState::Unplayable(why),
                    PlayerState::Reloading => {
                        self.state = PlayerState::Ready;
                        self.notice = Some(("new file cannot be played yet".into(), now));
                    }
                    _ => {}
                },
                PlayerEvent::Ended => match self.loop_range {
                    Some((start, _)) => {
                        p.seek(start);
                        p.play();
                    }
                    None => {
                        self.wanted_playing = false;
                        self.pause_player(p);
                    }
                },
            }
        }
    }

    fn ready(&mut self, p: &mut dyn Player, duration: f64, now: Instant) {
        let reloaded = self.state == PlayerState::Reloading;
        self.state = PlayerState::Ready;
        self.loaded = true;
        // AVFoundation reports an indefinite (NaN) duration for a file still being written.
        let duration = Some(duration).filter(|d| d.is_finite() && *d >= 0.0);
        if let Some(at) = self.restore_at.take() {
            p.seek(duration.map_or(at.max(0.0), |d| at.max(0.0).min(d)));
        }
        let mut cleared = false;
        if let (Some((start, end)), Some(duration)) = (self.loop_range, duration) {
            if start >= duration {
                self.loop_range = None;
                cleared = true;
            } else {
                self.loop_range = Some((start, end.min(duration)));
            }
            p.set_loop(self.loop_range);
        }
        if self.muted {
            p.set_muted(true);
        }
        if cleared {
            self.notice = Some(("loop cleared".into(), now));
        } else if reloaded {
            self.notice = Some(("new cut".into(), now));
        }
        if self.wanted_playing && !self.inactive {
            p.play();
            self.playing = true;
        } else {
            self.playing = false;
        }
    }

    fn pause_player(&mut self, p: &mut dyn Player) {
        if self.playing {
            p.pause();
            self.playing = false;
        }
    }

    pub fn play(&mut self, p: &mut dyn Player) {
        if matches!(self.state, PlayerState::Unplayable(_) | PlayerState::Waiting) {
            return;
        }
        self.wanted_playing = true;
        if self.inactive {
            return;
        }
        if let Some(d) = known(p.duration()) {
            if p.position() >= d - 0.05 {
                p.seek(0.0);
            }
        }
        p.play();
        self.playing = true;
    }

    pub fn pause(&mut self, p: &mut dyn Player) {
        self.wanted_playing = false;
        self.pause_player(p);
    }

    pub fn toggle(&mut self, p: &mut dyn Player) {
        if self.wanted_playing {
            self.pause(p);
        } else {
            self.play(p);
        }
    }

    pub fn seek(&mut self, p: &mut dyn Player, secs: f64) {
        let secs = secs.max(0.0);
        p.seek(known(p.duration()).map_or(secs, |d| secs.min(d)));
    }

    /// Steps frames, forward or back; stepping pauses.
    pub fn step(&mut self, p: &mut dyn Player, n: i32) {
        self.pause(p);
        p.step(n);
    }

    pub fn set_loop(&mut self, p: &mut dyn Player, change: LoopChange) {
        let dur = known(p.duration());
        match change {
            LoopChange::Keep => return,
            LoopChange::Set { start, end } => {
                let end = dur.map_or(end, |d| end.min(d));
                self.loop_range = (start < end).then_some((start.max(0.0), end));
            }
            LoopChange::WholeFile => match dur {
                Some(d) => self.loop_range = Some((0.0, d)),
                None => return,
            },
            LoopChange::Clear => self.loop_range = None,
        }
        p.set_loop(self.loop_range);
    }

    pub fn set_muted(&mut self, p: &mut dyn Player, muted: bool) {
        self.muted = muted;
        p.set_muted(muted);
    }

    /// A video plays only while it is the active tab of a shown window (spec 3.5): going inactive
    /// pauses, remembering what was wanted, and coming back resumes only that.
    pub fn set_inactive(&mut self, p: &mut dyn Player, inactive: bool) {
        if inactive == self.inactive {
            return;
        }
        self.inactive = inactive;
        if inactive {
            self.pause_player(p);
        } else if self.wanted_playing && self.state == PlayerState::Ready {
            p.play();
            self.playing = true;
        }
    }

    pub fn state_word(&self) -> &'static str {
        match self.state {
            PlayerState::Loading => "loading",
            PlayerState::Ready => "ready",
            PlayerState::Reloading => "reloading",
            PlayerState::Waiting => "waiting",
            PlayerState::Unplayable(_) => "unplayable",
        }
    }

    /// The notice to show at `now`, None once `NOTICE_FOR` has passed.
    pub fn notice_at(&self, now: Instant) -> Option<&str> {
        let (text, at) = self.notice.as_ref()?;
        (now.saturating_duration_since(*at) < NOTICE_FOR).then_some(text.as_str())
    }

    /// What the strip shows, and what `arkos video` reports.
    pub fn status(&self, p: &dyn Player, pane: u32, tab: u64, file_mtime: Option<u64>) -> VideoState {
        let (width, height) = p.size().unwrap_or((0, 0));
        VideoState {
            pane,
            tab,
            path: self.path.clone(),
            duration: known(p.duration()),
            position: p.position(),
            playing: self.playing,
            muted: self.muted,
            loop_range: self.loop_range,
            width,
            height,
            state: self.state_word().to_string(),
            error: match &self.state {
                PlayerState::Unplayable(why) => Some(why.clone()),
                _ => None,
            },
            file_mtime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(dur: f64) -> PlayerEvent {
        PlayerEvent::Ready { duration: dur, width: 1920, height: 1080 }
    }

    /// A model opened on /p/x.mp4 and made ready with a clip of `dur` seconds.
    fn opened(dur: f64) -> (VideoModel, FakePlayer, Instant) {
        let now = Instant::now();
        let mut m = VideoModel::new("/p/x.mp4".into(), None, None);
        let mut p = FakePlayer::default();
        m.open(&mut p, Some((1, 10)));
        p.pending.push(ready(dur));
        m.apply(&mut p, now);
        p.calls.clear();
        (m, p, now)
    }

    #[test]
    fn times_parse_in_every_form_and_refuse_nonsense() {
        assert_eq!(parse_time("12"), Ok(12.0));
        assert_eq!(parse_time("12.5"), Ok(12.5));
        assert_eq!(parse_time("0:12"), Ok(12.0));
        assert_eq!(parse_time("1:02:03.250"), Ok(3723.25));
        for bad in ["-1", "1:60", "abc", "", "1:2:3:4", "1:", ":5", "1e3", "inf", "1.2.3", "1:60:00"] {
            assert!(parse_time(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn times_format_with_tenths_and_hours() {
        assert_eq!(fmt_time(0.0), "0:00.0");
        assert_eq!(fmt_time(72.25), "1:12.2");
        assert_eq!(fmt_time(3725.0), "1:02:05.0");
        assert_eq!(fmt_time(-3.0), "0:00.0");
    }

    #[test]
    fn ranges_need_a_start_before_the_end() {
        assert_eq!(parse_range("12-18"), Ok((12.0, 18.0)));
        assert_eq!(parse_range("0:12-1:00.5"), Ok((12.0, 60.5)));
        assert!(parse_range("18-12").is_err());
        assert!(parse_range("12-12").is_err());
        assert!(parse_range("12").is_err());
        assert!(parse_range("a-b").is_err());
    }

    #[test]
    fn a_changing_file_waits_and_a_stable_one_reloads() {
        let (a, b, c) = (Some((1, 10)), Some((2, 20)), Some((3, 30)));
        assert_eq!(follow(a, a, a, false), Follow::Nothing);
        assert_eq!(follow(a, a, b, false), Follow::Nothing);
        assert_eq!(follow(a, b, b, false), Follow::Reload);
        assert_eq!(follow(a, a, None, false), Follow::Waiting);
        assert_eq!(follow(a, None, c, true), Follow::Nothing);
        assert_eq!(follow(a, c, c, true), Follow::Back);
        assert_eq!(follow(None, b, b, false), Follow::Reload);
    }

    #[test]
    fn a_failed_reload_keeps_the_old_item_and_says_so() {
        let (mut m, mut p, now) = opened(10.0);
        p.pos = 4.0;
        m.reload_now(&mut p);
        assert_eq!(m.state_word(), "reloading");
        p.pending.push(PlayerEvent::Failed("truncated".into()));
        m.apply(&mut p, now);
        assert_eq!(m.state, PlayerState::Ready);
        assert_eq!(p.position(), 4.0);
        assert_eq!(m.notice_at(now), Some("new file cannot be played yet"));
        assert_eq!(m.notice_at(now + NOTICE_FOR), None);
        assert_eq!(p.calls, vec!["load /p/x.mp4"]);
    }

    #[test]
    fn a_shorter_cut_clamps_the_position_and_clears_a_loop_past_its_end() {
        let (mut m, mut p, now) = opened(60.0);
        p.pos = 50.0;
        m.loop_range = Some((40.0, 48.0));
        m.reload_now(&mut p);
        p.pending.push(ready(30.0));
        m.apply(&mut p, now);
        assert_eq!(p.position(), 30.0);
        assert_eq!(m.loop_range, None);
        assert_eq!(m.notice_at(now), Some("loop cleared"));

        let (mut m, mut p, now) = opened(60.0);
        p.pos = 50.0;
        m.loop_range = Some((10.0, 48.0));
        m.reload_now(&mut p);
        p.pending.push(ready(30.0));
        m.apply(&mut p, now);
        assert_eq!(m.loop_range, Some((10.0, 30.0)));
        assert_eq!(m.notice_at(now), Some("new cut"));
        assert!(p.calls.contains(&"loop Some((10.0, 30.0))".to_string()));
    }

    #[test]
    fn a_reload_keeps_playing_what_was_playing() {
        let (mut m, mut p, now) = opened(60.0);
        m.play(&mut p);
        p.pos = 5.0;
        p.calls.clear();
        m.reload_now(&mut p);
        p.pending.push(ready(60.0));
        m.apply(&mut p, now);
        assert_eq!(p.calls, vec!["load /p/x.mp4", "seek 5.0", "play"]);
        assert!(m.playing);
    }

    #[test]
    fn switching_away_pauses_and_coming_back_resumes_only_what_was_playing() {
        let (mut m, mut p, _) = opened(60.0);
        m.play(&mut p);
        m.set_inactive(&mut p, true);
        assert!(!m.playing && m.wanted_playing);
        m.set_inactive(&mut p, false);
        assert!(m.playing);
        assert_eq!(p.calls, vec!["play", "pause", "play"]);

        m.pause(&mut p);
        p.calls.clear();
        m.set_inactive(&mut p, true);
        m.set_inactive(&mut p, false);
        assert!(p.calls.is_empty(), "a paused video stays paused: {:?}", p.calls);
        assert!(!m.playing);
    }

    #[test]
    fn a_missing_file_at_open_waits_then_loads_when_it_appears() {
        let now = Instant::now();
        let mut m = VideoModel::new("/p/x.mp4".into(), Some(7.0), None);
        let mut p = FakePlayer::default();
        m.open(&mut p, None);
        assert_eq!(m.state_word(), "waiting");
        assert!(p.calls.is_empty());
        assert!(!m.tick(&mut p, None, now));
        let fp = Some((5, 50));
        assert!(!m.tick(&mut p, fp, now), "one sighting is not stable");
        assert!(m.tick(&mut p, fp, now));
        assert_eq!(m.state_word(), "loading");
        assert_eq!(m.shown_fp, fp);
        p.pending.push(ready(20.0));
        m.apply(&mut p, now);
        assert_eq!(m.state_word(), "ready");
        assert_eq!(p.calls, vec!["load /p/x.mp4", "seek 7.0"]);
    }

    #[test]
    fn a_vanished_file_waits_and_pauses_then_the_next_stable_one_reloads() {
        let (mut m, mut p, now) = opened(60.0);
        let b = Some((2, 20));
        m.play(&mut p);
        assert!(!m.tick(&mut p, None, now));
        assert_eq!(m.state_word(), "waiting");
        assert!(!m.playing && m.wanted_playing);
        assert!(!m.tick(&mut p, b, now));
        assert!(m.tick(&mut p, b, now));
        assert_eq!(m.state_word(), "reloading");
    }

    #[test]
    fn ended_pauses_without_a_loop_and_seeks_to_the_start_of_one() {
        let (mut m, mut p, now) = opened(60.0);
        m.play(&mut p);
        p.pos = 60.0;
        p.pending.push(PlayerEvent::Ended);
        p.calls.clear();
        m.apply(&mut p, now);
        assert_eq!(p.calls, vec!["pause"]);
        assert!(!m.playing && !m.wanted_playing);
        p.calls.clear();
        m.play(&mut p);
        assert_eq!(p.calls, vec!["seek 0.0", "play"], "play at the end starts over");

        m.set_loop(&mut p, LoopChange::Set { start: 12.0, end: 18.0 });
        p.pending.push(PlayerEvent::Ended);
        p.calls.clear();
        m.apply(&mut p, now);
        assert_eq!(p.calls, vec!["seek 12.0", "play"]);
        assert!(m.playing);
    }

    #[test]
    fn loops_seeks_and_steps_are_clamped_and_passed_on() {
        let (mut m, mut p, _) = opened(30.0);
        m.set_loop(&mut p, LoopChange::Set { start: 12.0, end: 99.0 });
        assert_eq!(m.loop_range, Some((12.0, 30.0)));
        m.set_loop(&mut p, LoopChange::Keep);
        assert_eq!(m.loop_range, Some((12.0, 30.0)));
        m.set_loop(&mut p, LoopChange::WholeFile);
        assert_eq!(m.loop_range, Some((0.0, 30.0)));
        m.set_loop(&mut p, LoopChange::Clear);
        assert_eq!(m.loop_range, None);
        m.seek(&mut p, 99.0);
        m.seek(&mut p, -4.0);
        m.play(&mut p);
        m.step(&mut p, 1);
        m.set_muted(&mut p, true);
        assert_eq!(
            p.calls,
            vec![
                "loop Some((12.0, 30.0))",
                "loop Some((0.0, 30.0))",
                "loop None",
                "seek 30.0",
                "seek 0.0",
                "play",
                "pause",
                "step 1",
                "muted true"
            ]
        );
        assert!(!m.wanted_playing, "stepping pauses");
    }

    #[test]
    fn a_first_load_that_fails_is_unplayable_with_the_reason() {
        let now = Instant::now();
        let mut m = VideoModel::new("/p/x.mp4".into(), None, None);
        let mut p = FakePlayer::default();
        m.open(&mut p, Some((1, 10)));
        p.pending.push(PlayerEvent::Failed("no video track".into()));
        m.apply(&mut p, now);
        let s = m.status(&p, 1, 2, None);
        assert_eq!((s.state.as_str(), s.error.as_deref()), ("unplayable", Some("no video track")));
    }

    #[test]
    fn status_reports_what_the_strip_shows() {
        let (mut m, mut p, _) = opened(60.0);
        m.play(&mut p);
        m.set_muted(&mut p, true);
        m.set_loop(&mut p, LoopChange::Set { start: 12.0, end: 18.0 });
        p.pos = 14.5;
        let s = m.status(&p, 3, 9, Some(1234));
        assert_eq!(
            s,
            VideoState {
                pane: 3,
                tab: 9,
                path: "/p/x.mp4".into(),
                duration: Some(60.0),
                position: 14.5,
                playing: true,
                muted: true,
                loop_range: Some((12.0, 18.0)),
                width: 1920,
                height: 1080,
                state: "ready".into(),
                error: None,
                file_mtime: Some(1234),
            }
        );
    }

    #[test]
    fn an_unchanged_file_is_not_reloaded_by_the_polls_after_opening() {
        let now = Instant::now();
        let a = Some((1, 10));
        let mut m = VideoModel::new("/p/x.mp4".into(), None, None);
        let mut p = FakePlayer::default();
        m.open(&mut p, a);
        p.pending.push(ready(20.0));
        m.apply(&mut p, now);
        p.calls.clear();
        assert!(!m.tick(&mut p, a, now));
        assert!(!m.tick(&mut p, a, now));
        assert!(p.calls.is_empty(), "{:?}", p.calls);
        assert_eq!(m.notice, None);
        m.reload_now(&mut p);
        p.pending.push(ready(20.0));
        assert!(!m.tick(&mut p, a, now), "a manual reload is not repeated by the next poll");
    }

    #[test]
    fn an_unknown_or_negative_duration_is_survived_as_unknown() {
        for bad in [f64::NAN, -1.0, f64::INFINITY] {
            let now = Instant::now();
            let mut m = VideoModel::new("/p/x.mp4".into(), Some(5.0), Some((1.0, 2.0)));
            let mut p = FakePlayer::default();
            m.open(&mut p, Some((1, 10)));
            p.pending.push(ready(bad));
            m.apply(&mut p, now);
            let s = m.status(&p, 1, 1, None);
            assert_eq!((s.state.as_str(), s.duration), ("ready", None), "duration {bad}");
            assert_eq!(m.loop_range, Some((1.0, 2.0)));
            assert!(p.calls.contains(&"seek 5.0".to_string()));
            m.seek(&mut p, 3.0);
        }
    }
}
