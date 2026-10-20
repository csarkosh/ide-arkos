//! The video tab's view (spec 3.2 to 3.5): one per video tab, holding the playback model, the
//! player behind it, the frame on screen and the scrubber's drag. `video_element` draws it; this
//! file has the state and the actions the keys, the strip and an agent's commands run, each
//! through the model and then a repaint. The model is tested headless in `video.rs`; nothing here
//! is tested through gpui (the crate has no gpui test harness), and the pure pieces of the strip
//! are tested in `video_element.rs`.

use crate::app_nap::{keep_display_awake, DisplayAwake};
use crate::video::{Fp, Player, PlayerState, VideoModel};
use core_foundation::base::TCFType;
use core_video::pixel_buffer::CVPixelBuffer;
use gpui::{Bounds, Context, EventEmitter, FocusHandle, Pixels};
use protocol::LoopChange;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How far Left and Right move, and Shift with them, in seconds (spec 3.4).
pub const SKIP: f64 = 5.0;
pub const SKIP_FINE: f64 = 1.0;

/// How long a paused view keeps asking for display frames after a seek, a step or a load, while
/// it waits for the frame the player copies once the seek has landed. The first new frame ends
/// the wait sooner.
const SETTLE_FOR: Duration = Duration::from_millis(500);

/// A selection shorter than this, in seconds, is a click, not a range.
const MIN_SELECTION: f64 = 0.05;

/// The scrubber while the mouse holds it, and the range a Shift+drag selected once it is let go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scrub {
    /// The button is down on the scrubber; the drag seeks as it moves.
    pub dragging: bool,
    /// The video was playing when the press came, so it plays again when the drag ends.
    pub was_playing: bool,
    /// Shift was down at the press: the drag selects a range from this time, which L loops.
    pub select_from: Option<f64>,
    /// Where the drag is now (the selection's other end).
    pub at: f64,
}

/// What `Root` hears from a view: the position or the loop changed in a way the workspace keeps
/// (a pause, a seek, a loop), so it writes them into the tab and saves.
pub enum VideoEvent {
    StateChanged,
}

impl EventEmitter<VideoEvent> for VideoView {}

pub struct VideoView {
    pub model: VideoModel,
    player: Box<dyn Player>,
    /// The frame on screen, kept until the player has a newer one.
    pub frame: Option<CVPixelBuffer>,
    /// The file's Quick Look poster when the media cache already has it, shown before the first
    /// frame.
    pub poster: Option<PathBuf>,
    /// Keeps the display awake while the video plays in an active tab.
    awake: Option<DisplayAwake>,
    pub scrub: Option<Scrub>,
    pub focus: FocusHandle,
    /// The position the tab was saved at, reported by `saved_state` until the file has loaded.
    at: Option<f64>,
    /// Until when a paused view asks for frames (`SETTLE_FOR`).
    settle: Option<Instant>,
    /// The scrubber's track as last laid out, in window pixels; a press and a drag are measured
    /// against it.
    pub track: Option<Bounds<Pixels>>,
    /// The notice a repaint has been scheduled for, so its end is drawn without a frame loop.
    notice_timer: Option<Instant>,
    /// Whether this tab has shown a frame yet, and whether its failure was recorded (telemetry).
    shown: bool,
    failure_recorded: bool,
}

/// The poster `media` made for `path` as it is now, when the cache has it.
fn cached_poster(path: &Path) -> Option<PathBuf> {
    let mtime = crate::media::mtime_secs(path)?;
    let poster = crate::media::poster_path(&crate::media::poster_dir(), path, mtime);
    crate::cache::fresh(&poster, crate::cache::POSTERS_AGE).then_some(poster)
}

/// A pixel buffer's identity, to tell a new frame from the one shown.
fn buffer_id(b: &CVPixelBuffer) -> usize {
    b.as_concrete_TypeRef() as usize
}

impl VideoView {
    /// A view on `path` that starts loading it now (or waits for it, when it is missing), to come
    /// up paused at `at` with `loop_range` set.
    pub fn new(path: PathBuf, at: Option<f64>, loop_range: Option<(f64, f64)>, mut player: Box<dyn Player>, cx: &mut Context<Self>) -> Self {
        let mut model = VideoModel::new(path.clone(), at, loop_range);
        model.open(&mut *player, crate::config_watch::fingerprint(&path));
        VideoView {
            model,
            player,
            frame: None,
            poster: cached_poster(&path),
            awake: None,
            scrub: None,
            focus: cx.focus_handle(),
            at,
            settle: None,
            track: None,
            notice_timer: None,
            shown: false,
            failure_recorded: false,
        }
    }

    /// The current position, in seconds.
    pub fn position(&self) -> f64 {
        self.player.position()
    }

    /// The length, when the player knows it.
    pub fn duration(&self) -> Option<f64> {
        self.player.duration().filter(|d| d.is_finite() && *d > 0.0)
    }

    /// The range a Shift+drag on the scrubber selected (or is selecting), start first.
    pub fn selection(&self) -> Option<(f64, f64)> {
        let s = self.scrub?;
        let from = s.select_from?;
        Some((from.min(s.at), from.max(s.at)))
    }

    fn loading(&self) -> bool {
        matches!(self.model.state, PlayerState::Loading | PlayerState::Reloading)
    }

    /// Asks for frames for a moment: a paused seek's frame arrives a little after the call.
    fn settle(&mut self) {
        self.settle = Some(Instant::now() + SETTLE_FOR);
    }

    /// The display stays awake exactly while the video plays in an active tab.
    fn sync_awake(&mut self) {
        let wanted = self.model.playing && !self.model.inactive;
        if wanted && self.awake.is_none() {
            self.awake = Some(keep_display_awake());
        } else if !wanted {
            self.awake = None;
        }
    }

    /// After an action: the token follows the model, and the strip is drawn again.
    fn after(&mut self, cx: &mut Context<Self>) {
        self.sync_awake();
        cx.notify();
    }

    /// Called by the element on every draw: takes what the player reports, the newest frame and
    /// the display token. True when the next display frame should be drawn too: while playing,
    /// while loading, and for a moment after a seek, a step or a load until its frame arrives.
    pub fn frame_step(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        let (was_loading, was_wanted) = (self.loading(), self.model.wanted_playing);
        self.model.apply(&mut *self.player, now);
        if was_loading && !self.loading() {
            // The new item's first frame (or the old one's, put back) is copied after its seek.
            self.settle = Some(now + SETTLE_FOR);
        }
        if was_wanted && !self.model.wanted_playing {
            // It played to the end: the position is worth keeping.
            cx.emit(VideoEvent::StateChanged);
        }
        if let PlayerState::Unplayable(_) = self.model.state {
            if !self.failure_recorded {
                self.failure_recorded = true;
                crate::telemetry::error(crate::telemetry::Failure::VideoUnplayable);
            }
        }
        if let Some(frame) = self.player.frame() {
            if self.frame.as_ref().map(buffer_id) != Some(buffer_id(&frame)) {
                self.frame = Some(frame);
                self.settle = None;
                if !self.shown {
                    self.shown = true;
                    crate::telemetry::video_shown();
                }
            }
        }
        self.schedule_notice_end(cx);
        self.sync_awake();
        let settling = self.settle.is_some_and(|until| now < until);
        if !settling {
            self.settle = None;
        }
        self.model.playing || self.loading() || settling
    }

    /// A notice ("new cut") leaves the strip after `NOTICE_FOR`; a paused view asks for no frames,
    /// so one repaint is scheduled for then.
    fn schedule_notice_end(&mut self, cx: &mut Context<Self>) {
        let Some((_, at)) = self.model.notice.as_ref() else { return };
        let at = *at;
        if self.notice_timer == Some(at) {
            return;
        }
        self.notice_timer = Some(at);
        let left = crate::video::NOTICE_FOR.saturating_sub(at.elapsed());
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(left).await;
            let _ = view.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// The once a second poll of the file (spec 4): `fp` is its fingerprint now.
    // `Root::paths_changed` calls it from Task 7 on.
    #[allow(dead_code)]
    pub fn tick(&mut self, fp: Fp, cx: &mut Context<Self>) {
        let before = (self.model.state.clone(), self.model.notice.clone());
        self.model.tick(&mut *self.player, fp, Instant::now());
        if before != (self.model.state.clone(), self.model.notice.clone()) {
            self.after(cx);
        } else {
            self.sync_awake();
        }
    }

    /// A video plays only while it is the active tab of a shown pane (spec 3.5): going inactive
    /// pauses and remembers, coming back resumes only what was playing.
    pub fn set_inactive(&mut self, inactive: bool, cx: &mut Context<Self>) {
        if inactive == self.model.inactive {
            return;
        }
        let was_playing = self.model.playing;
        self.model.set_inactive(&mut *self.player, inactive);
        if inactive && was_playing {
            cx.emit(VideoEvent::StateChanged);
        }
        self.after(cx);
    }

    /// The position and the loop to keep in the tab (`Tree::set_video_state`).
    pub fn saved_state(&self) -> (Option<f64>, Option<(f64, f64)>) {
        let at = match self.model.state {
            PlayerState::Ready | PlayerState::Reloading => Some(self.player.position()),
            _ => self.at,
        };
        (at, self.model.loop_range)
    }

    /// What `arkos video status` reports for this tab.
    // The targeted channel answers with it from Task 8 on.
    #[allow(dead_code)]
    pub fn status(&self, pane: u32, tab: u64) -> protocol::VideoState {
        self.model.status(&*self.player, pane, tab, crate::media::mtime_secs(&self.model.path))
    }

    pub fn play_pause(&mut self, cx: &mut Context<Self>) {
        self.model.toggle(&mut *self.player);
        if !self.model.wanted_playing {
            cx.emit(VideoEvent::StateChanged);
        }
        self.after(cx);
    }

    pub fn play(&mut self, cx: &mut Context<Self>) {
        self.model.play(&mut *self.player);
        self.after(cx);
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        self.model.pause(&mut *self.player);
        cx.emit(VideoEvent::StateChanged);
        self.after(cx);
    }

    pub fn seek(&mut self, secs: f64, cx: &mut Context<Self>) {
        self.model.seek(&mut *self.player, secs);
        self.settle();
        cx.emit(VideoEvent::StateChanged);
        self.after(cx);
    }

    pub fn back(&mut self, secs: f64, cx: &mut Context<Self>) {
        let at = self.player.position() - secs;
        self.seek(at, cx);
    }

    pub fn forward(&mut self, secs: f64, cx: &mut Context<Self>) {
        let at = self.player.position() + secs;
        self.seek(at, cx);
    }

    pub fn to_start(&mut self, cx: &mut Context<Self>) {
        self.seek(0.0, cx);
    }

    /// `n` frames forward (or back when negative), pausing first.
    pub fn step(&mut self, n: i32, cx: &mut Context<Self>) {
        self.model.step(&mut *self.player, n);
        self.settle();
        cx.emit(VideoEvent::StateChanged);
        self.after(cx);
    }

    pub fn set_loop(&mut self, change: LoopChange, cx: &mut Context<Self>) {
        self.model.set_loop(&mut *self.player, change);
        cx.emit(VideoEvent::StateChanged);
        self.after(cx);
    }

    /// L: loops the scrubber's selection when there is one; otherwise on loops the whole file and
    /// off clears the loop.
    pub fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        let change = match self.selection().filter(|_| !self.scrub.is_some_and(|s| s.dragging)) {
            Some((start, end)) => {
                self.scrub = None;
                LoopChange::Set { start, end }
            }
            None if self.model.loop_range.is_some() => LoopChange::Clear,
            None => LoopChange::WholeFile,
        };
        self.set_loop(change, cx);
    }

    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        self.model.set_muted(&mut *self.player, muted);
        self.after(cx);
    }

    pub fn mute(&mut self, cx: &mut Context<Self>) {
        let muted = !self.model.muted;
        self.set_muted(muted, cx);
    }

    /// R: loads the file again now, keeping the position, the rate and the loop.
    pub fn reload_now(&mut self, cx: &mut Context<Self>) {
        self.model.reload_now(&mut *self.player);
        self.after(cx);
    }

    /// The time under window x `x` on the scrubber's track.
    fn time_at(&self, x: f32) -> f64 {
        let (Some(track), Some(duration)) = (self.track, self.duration()) else { return self.player.position() };
        crate::video_element::scrub_time(x - f32::from(track.origin.x), duration, f32::from(track.size.width))
    }

    /// A press on the scrubber: pauses for the drag and seeks there; with Shift it also starts a
    /// selection.
    pub fn scrub_press(&mut self, x: f32, shift: bool, cx: &mut Context<Self>) {
        if self.duration().is_none() {
            return;
        }
        let at = self.time_at(x);
        let was_playing = self.model.playing;
        if was_playing {
            self.model.pause(&mut *self.player);
        }
        self.model.seek(&mut *self.player, at);
        self.settle();
        self.scrub = Some(Scrub { dragging: true, was_playing, select_from: shift.then_some(at), at });
        self.after(cx);
    }

    /// The pointer moved while the scrubber is held: seek there. A move with the button up is a
    /// release that was missed.
    pub fn scrub_move(&mut self, x: f32, held: bool, cx: &mut Context<Self>) {
        if !self.scrub.is_some_and(|s| s.dragging) {
            return;
        }
        if !held {
            self.scrub_release(cx);
            return;
        }
        let at = self.time_at(x);
        if let Some(s) = self.scrub.as_mut() {
            s.at = at;
        }
        self.model.seek(&mut *self.player, at);
        self.settle();
        self.after(cx);
    }

    /// The button came up: playing resumes when it was playing, and a selection stays for L.
    pub fn scrub_release(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.scrub.filter(|s| s.dragging) else { return };
        if s.was_playing {
            self.model.play(&mut *self.player);
        }
        self.scrub = s.select_from.filter(|from| (s.at - from).abs() >= MIN_SELECTION).map(|_| Scrub { dragging: false, ..s });
        cx.emit(VideoEvent::StateChanged);
        self.after(cx);
    }
}
