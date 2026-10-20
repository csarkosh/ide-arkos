//! The video tab's real player: AVFoundation behind `video::Player` (spec 3.2). An `AVPlayer`
//! plays an `AVPlayerItem` on an `AVURLAsset` of the file, and AVFoundation plays the audio
//! itself. One `AVPlayerItemVideoOutput`, made once and moved from item to item, hands back the
//! frames as CoreVideo pixel buffers for gpui's `surface()` element; Arkos never touches pixels.
//!
//! The output's pixel buffer attributes are mandatory, not a nicety:
//! `kCVPixelBufferPixelFormatTypeKey = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` (`'420f'`),
//! `kCVPixelBufferIOSurfacePropertiesKey = {}` (IOSurface backed) and
//! `kCVPixelBufferMetalCompatibilityKey = true`. gpui 0.2.2's Metal renderer
//! (`platform/mac/metal_renderer.rs`, `draw_surfaces`) asserts exactly that pixel format for a
//! surface and `unwrap()`s `CVMetalTextureCache::create_texture_from_image` on each of the two
//! planes, so a buffer in another format, or one that is not IOSurface backed and Metal
//! compatible, panics the window.
//!
//! Everything here runs on the main thread: `AVPlayer` and `AVPlayerItem` are main thread only in
//! the binding, `new` takes the marker, and the view that owns the player (Task 6) lives on gpui's
//! single UI thread. Readiness, the end and the loop are found by polling (`poll`, and `frame`
//! for the loop) rather than by key value observing, so no callback reaches into this state; the
//! one callback, a seek's completion handler, only bumps an atomic counter. Nothing in
//! `cargo test` constructs a player.
#![cfg(target_os = "macos")]

use crate::video::{Player, PlayerEvent};
use block2::RcBlock;
use core_foundation::base::TCFType;
use core_video::pixel_buffer::{CVPixelBuffer, CVPixelBufferRef};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{AnyThread, MainThreadMarker};
use objc2_av_foundation::{
    AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem, AVPlayerItemStatus, AVPlayerItemVideoOutput, AVURLAsset,
};
use objc2_core_foundation::CFString;
use objc2_core_media::{kCMTimeZero, CMTime};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// From `QuartzCore/CABase.h`: the host time in seconds, the clock `itemTimeForHostTime` expects.
// Linked directly, as ax.rs links ApplicationServices, rather than adding the QuartzCore binding.
#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    fn CACurrentMediaTime() -> f64;
}

/// The timescale seeks are made in: the 90 kHz MPEG clock, finer than any frame rate.
const TIMESCALE: i32 = 90_000;

/// How close to the end (or to a loop's end) a paused player counts as having reached it: a
/// frame at 10 fps, longer than one frame of any cut.
const END_SLACK: f64 = 0.1;

/// What `poll` has found out about the item on the player.
///
/// The reload rule: `held` and `resume_rate` are taken by a `load` only while the phase is
/// `Ready`, so they describe the item a failed reload falls back to; a second `load` during
/// `Loading` keeps the ones it has (the player is paused by then, so its rate would read zero).
/// While `Loading` with a `shown` item to fall back to, `play` and `pause` record the intent in
/// `resume_rate` (1.0 or 0.0) instead of messaging the player: playing would start the new item
/// from zero, and a pause during a reload must hold if the old item comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Loaded but not yet ready or failed; nothing reported.
    Loading,
    /// `Ready` reported; this item is `shown`.
    Ready,
    /// `Failed` reported, with no ready item to go back to.
    Failed,
}

/// An `AVPlayer` with its current item and the video output, the last frame copied, and the
/// state the polls need (the loop, whether it was asked to play, the seeks in flight).
pub struct AvPlayer {
    mtm: MainThreadMarker,
    player: Retained<AVPlayer>,
    /// The item on the player: the one being loaded, or the one shown once it is ready.
    item: Option<Retained<AVPlayerItem>>,
    phase: Phase,
    /// The last item that became ready. While a reload loads, this is the old item (off the
    /// player), kept so a new item that fails can be put back and the old one keeps playing.
    shown: Option<Retained<AVPlayerItem>>,
    /// Made once with the mandatory attributes (module doc) and moved to each new item.
    output: Retained<AVPlayerItemVideoOutput>,
    /// The last pixel buffer copied, handed out again until a newer one exists.
    last: Option<CVPixelBuffer>,
    /// The position of the shown item when a reload began (see `Phase`), reported until the
    /// new item is ready and restored by a fall back.
    held: f64,
    /// The shown item's rate when a reload began, then the intent of any `play` or `pause` made
    /// during the reload (see `Phase`), restored when a failed reload puts the old item back.
    resume_rate: f32,
    /// Asked to play and not since paused or ended, so a stop at the end is an `Ended`.
    playing: bool,
    loop_range: Option<(f64, f64)>,
    /// A paused seek, step or load wants one frame copied on the next `frame` call.
    frame_wanted: bool,
    /// Seeks started on the current item, and how many of their completion handlers have run
    /// (the handler may run on any thread, so it only bumps this counter). A new counter is made
    /// per load, so a handler from a replaced item never counts for the new one.
    seeks_sent: u64,
    seeks_landed: Arc<AtomicU64>,
}

impl AvPlayer {
    /// A player with no item yet. Panics off the main thread, which would be a bug in the caller:
    /// the binding makes `AVPlayer` main thread only.
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("the video player is made on the main thread");
        // SAFETY: on the main thread (`mtm`); `new` has no other requirement.
        let player = unsafe { AVPlayer::new(mtm) };
        // At the end the player pauses (rate 0) and `poll` reports `Ended`; a loop is wrapped by
        // `poll` and `frame` seeking back. A plain `AVPlayer` raises on `Advance`, so not that.
        // SAFETY: a valid value for a plain `AVPlayer` (spec 3.2), on the main thread.
        unsafe { player.setActionAtItemEnd(AVPlayerActionAtItemEnd::Pause) };
        let attributes = output_attributes();
        // SAFETY: the dictionary holds the documented key and value types of `CVPixelBuffer.h`
        // (an `NSNumber` format, an `NSDictionary` of IOSurface properties, an `NSNumber`
        // boolean), which is what the method's generic parameter requires.
        let output = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(AVPlayerItemVideoOutput::alloc(), Some(&attributes))
        };
        AvPlayer {
            mtm,
            player,
            item: None,
            phase: Phase::Loading,
            shown: None,
            output,
            last: None,
            held: 0.0,
            resume_rate: 0.0,
            playing: false,
            loop_range: None,
            frame_wanted: false,
            seeks_sent: 0,
            seeks_landed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The newest frame for now, when the output has one; the last one otherwise. Also wraps a
    /// loop, since this is called every display frame while the video plays and `poll` only once
    /// a second.
    pub fn frame(&mut self) -> Option<CVPixelBuffer> {
        self.wrap_loop();
        // While a reload loads, the old item's last frame stays up: the new item's first frames
        // (at zero, before the model seeks it back to where the old one was) are not shown.
        if self.phase != Phase::Ready {
            return self.last.clone();
        }
        // SAFETY: a plain C function with no arguments, callable from any thread.
        let host = unsafe { CACurrentMediaTime() };
        // SAFETY: the output is live and on the current item; called on the main thread. Paused,
        // the item time is the current time.
        let time = unsafe { self.output.itemTimeForHostTime(host) };
        // SAFETY: as above; `time` came from the output itself.
        let fresh = unsafe { self.output.hasNewPixelBufferForItemTime(time) };
        // After a paused seek has landed (or a step or a load), copy once even when the output
        // says nothing is new, in case it had suspended itself while paused (spec 3.2).
        let forced = self.frame_wanted && !self.seeking();
        if fresh || forced {
            // SAFETY: `out_item_time_for_display` may be null (documented); the output is live.
            // A `None` means nothing new to show for this time: the last frame stays.
            if let Some(buffer) = unsafe { self.output.copyPixelBufferForItemTime_itemTimeForDisplay(time, ptr::null_mut()) }
            {
                self.last = Some(rewrap(buffer));
                self.frame_wanted = false;
            }
        }
        self.last.clone()
    }

    /// The player's rate: zero when paused or stopped at the end.
    pub fn rate(&self) -> f32 {
        // SAFETY: a property read on a live player, on the main thread.
        unsafe { self.player.rate() }
    }

    /// True while a reload loads with a ready item to fall back to (see `Phase`).
    fn reloading(&self) -> bool {
        self.phase == Phase::Loading && self.shown.is_some()
    }

    /// True while a seek on the current item has not yet completed.
    fn seeking(&self) -> bool {
        self.seeks_landed.load(Ordering::Acquire) < self.seeks_sent
    }

    /// Wakes a paused output and marks that the next `frame` should copy one frame.
    fn want_frame(&mut self) {
        // SAFETY: a plain message to a live output, on the main thread. No delegate is set, so
        // nothing is called back; the request only takes the output out of its quiescent state.
        unsafe { self.output.requestNotificationOfMediaDataChangeWithAdvanceInterval(0.0) };
        self.frame_wanted = true;
    }

    /// Seeks back to the loop's start once playing reaches its end (or the player stopped at the
    /// file's end inside the loop). Nothing while a seek is in flight, so one wrap is one seek.
    fn wrap_loop(&mut self) {
        let Some((start, end)) = self.loop_range else {
            return;
        };
        if !self.playing || self.phase != Phase::Ready || self.seeking() {
            return;
        }
        let pos = self.position();
        let stopped = self.rate() == 0.0;
        if pos >= end || (stopped && pos >= end - END_SLACK) {
            self.seek(start);
            if stopped {
                // SAFETY: a plain message to a live player, on the main thread.
                unsafe { self.player.play() };
            }
        }
    }

    /// True once, when a player that was asked to play has stopped at the end of the file.
    fn ended(&self) -> bool {
        let Some(duration) = self.duration().filter(|d| d.is_finite()) else {
            return false;
        };
        self.playing
            && self.phase == Phase::Ready
            && !self.seeking()
            && self.rate() == 0.0
            && self.position() >= duration - END_SLACK
    }

    /// The item being loaded failed: puts the last ready item back on the player at the position
    /// and rate it had, or, on a first load, leaves the failed item and reports nothing more.
    fn fall_back(&mut self) {
        let Some(old) = self.shown.clone() else {
            self.phase = Phase::Failed;
            return;
        };
        if let Some(failed) = self.item.take() {
            // SAFETY: the output was added to this item by `load`; removing it lets it be added to
            // the old item (an output may belong to one item at a time). Main thread.
            unsafe { failed.removeOutput(&self.output) };
        }
        // SAFETY: the old item was this player's before, and is on no other player. The output was
        // just removed from the failed item, so it belongs to no item. Main thread.
        unsafe {
            old.addOutput(&self.output);
            self.player.replaceCurrentItemWithPlayerItem(Some(&old));
        }
        self.item = Some(old);
        self.phase = Phase::Ready;
        // A fresh counter, as in `load`: a handler of a seek on the failed item does not count.
        self.seeks_sent = 0;
        self.seeks_landed = Arc::new(AtomicU64::new(0));
        // The player is paused here (since `load`), so the seek also asks for the one frame.
        self.seek(self.held);
        if self.resume_rate != 0.0 {
            // SAFETY: a plain message to a live player, on the main thread.
            unsafe { self.player.setRate(self.resume_rate) };
            self.playing = true;
        }
    }
}

impl Player for AvPlayer {
    fn load(&mut self, path: &Path) {
        // Only a ready item's position and rate are worth going back to (see `Phase`).
        if self.phase == Phase::Ready {
            self.held = self.position();
            self.resume_rate = self.rate();
        }
        // The new item is paused until the model plays it once ready (after seeking it to the
        // old position), so it does not start from zero on its own.
        // SAFETY: a plain message to a live player, on the main thread.
        unsafe { self.player.pause() };
        self.playing = false;
        // APFS file names are UTF-8, so the lossy conversion loses nothing for a real path.
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // SAFETY: no options (`None` is allowed); the URL is a live file URL.
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        // SAFETY: a live asset, on the main thread (`mtm`).
        let item = unsafe { AVPlayerItem::playerItemWithAsset(&asset, self.mtm) };
        if let Some(current) = &self.item {
            // SAFETY: the output is on the current item (added by an earlier `load` or
            // `fall_back`); it must leave it before it can be added to the new one. Main thread.
            unsafe { current.removeOutput(&self.output) };
        }
        // SAFETY: the output belongs to no item now, and the new item is on no player. The output
        // is added before the item goes on the player, as Apple's samples do. Main thread.
        unsafe {
            item.addOutput(&self.output);
            self.player.replaceCurrentItemWithPlayerItem(Some(&item));
        }
        self.item = Some(item);
        self.phase = Phase::Loading;
        self.frame_wanted = false;
        self.seeks_sent = 0;
        self.seeks_landed = Arc::new(AtomicU64::new(0));
    }

    fn play(&mut self) {
        if self.reloading() {
            // Recorded for a fall back (see `Phase`); the model plays the new item once ready.
            self.resume_rate = 1.0;
            return;
        }
        // SAFETY: a plain message to a live player, on the main thread. Before a first item is
        // ready this sets the rate and the item starts once it can.
        unsafe { self.player.play() };
        self.playing = true;
    }

    fn pause(&mut self) {
        if self.reloading() {
            // The player is already paused by `load`; a fall back must stay paused.
            self.resume_rate = 0.0;
            return;
        }
        // SAFETY: a plain message to a live player, on the main thread.
        unsafe { self.player.pause() };
        self.playing = false;
    }

    fn seek(&mut self, secs: f64) {
        // A time that is not a number would make an invalid `CMTime`, which AVFoundation rejects
        // with an exception; the model never sends one, but a bad value must not crash the window.
        if !secs.is_finite() {
            return;
        }
        // SAFETY: plain C arithmetic on a finite number of seconds; `kCMTimeZero` is a framework
        // constant, valid for the process.
        let (time, zero) = unsafe { (CMTime::with_seconds(secs.max(0.0), TIMESCALE), kCMTimeZero) };
        self.seeks_sent += 1;
        let landed = Arc::clone(&self.seeks_landed);
        // Called once per seek, finished or interrupted, on a thread AVFoundation picks: it only
        // bumps the shared counter, which is `Send` and `Sync`.
        let done = RcBlock::new(move |_finished: Bool| {
            landed.fetch_add(1, Ordering::Release);
        });
        // SAFETY: zero tolerances ask for a frame exact seek; the handler is sendable (it captures
        // only an `Arc<AtomicU64>`), as the binding's safety note requires. Main thread.
        unsafe { self.player.seekToTime_toleranceBefore_toleranceAfter_completionHandler(time, zero, zero, &done) };
        if self.rate() == 0.0 {
            self.want_frame();
        }
    }

    fn step(&mut self, frames: i32) {
        if frames == 0 || self.phase != Phase::Ready {
            return;
        }
        let Some(item) = self.item.clone() else {
            return;
        };
        self.want_frame();
        // SAFETY: a plain message to the live current item, on the main thread (required before
        // macOS 13). Stepping leaves the rate at zero; the model pauses first.
        unsafe { item.stepByCount(frames as isize) };
    }

    fn set_muted(&mut self, muted: bool) {
        // SAFETY: a property write on a live player, on the main thread.
        unsafe { self.player.setMuted(muted) };
    }

    fn set_loop(&mut self, range: Option<(f64, f64)>) {
        self.loop_range = range;
    }

    fn position(&self) -> f64 {
        if self.phase != Phase::Ready {
            return self.held;
        }
        // SAFETY: a property read on a live player, then plain C arithmetic. An invalid or
        // indefinite time comes back as NaN, which falls back to the held position.
        let secs = unsafe { self.player.currentTime().seconds() };
        if secs.is_finite() {
            secs.max(0.0)
        } else {
            self.held
        }
    }

    /// The shown item's duration, passed through as AVFoundation gives it: NaN for an indefinite
    /// one (a file still being written), which the model treats as unknown.
    fn duration(&self) -> Option<f64> {
        // SAFETY: a property read on a live item, then plain C arithmetic. Main thread.
        self.shown.as_ref().map(|item| unsafe { item.duration().seconds() })
    }

    fn size(&self) -> Option<(u32, u32)> {
        // SAFETY: a property read on a live item, on the main thread.
        let size = unsafe { self.shown.as_ref()?.presentationSize() };
        pixels(size.width, size.height)
    }

    fn poll(&mut self) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        if let (Some(item), Phase::Loading) = (self.item.clone(), self.phase) {
            // SAFETY: a property read on a live item, on the main thread.
            let status = unsafe { item.status() };
            if status == AVPlayerItemStatus::ReadyToPlay {
                // SAFETY: property reads on a live, ready item, then plain C arithmetic.
                let (duration, size) = unsafe { (item.duration().seconds(), item.presentationSize()) };
                // A ready item with no presentation size has no visual part: an audio file.
                match pixels(size.width, size.height) {
                    Some((width, height)) => {
                        self.phase = Phase::Ready;
                        self.shown = Some(item);
                        self.want_frame();
                        events.push(PlayerEvent::Ready { duration, width, height });
                    }
                    None => {
                        self.fall_back();
                        events.push(PlayerEvent::Failed("no video track".into()));
                    }
                }
            } else if status == AVPlayerItemStatus::Failed {
                // SAFETY: a property read on a live, failed item, on the main thread.
                let why = unsafe { item.error() }
                    .map(|e| format!("cannot play: {}", e.localizedDescription()))
                    .unwrap_or_else(|| "cannot play".into());
                self.fall_back();
                events.push(PlayerEvent::Failed(why));
            }
        }
        self.wrap_loop();
        if self.ended() {
            self.playing = false;
            events.push(PlayerEvent::Ended);
        }
        events
    }

    fn frame(&mut self) -> Option<CVPixelBuffer> {
        AvPlayer::frame(self)
    }
}

impl Drop for AvPlayer {
    /// Closing the tab releases the player at once (spec 3.5): the sound stops now, even if
    /// AVFoundation keeps its own reference to the player a little longer.
    fn drop(&mut self) {
        // SAFETY: plain messages to the live player and item, on the main thread (the owner is
        // dropped where it lives). The output is taken off the item before the item leaves.
        unsafe {
            self.player.pause();
            if let Some(item) = &self.item {
                item.removeOutput(&self.output);
            }
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
    }
}

/// A presentation size in whole pixels, or None when it is empty (no video track).
fn pixels(width: f64, height: f64) -> Option<(u32, u32)> {
    (width >= 1.0 && height >= 1.0).then(|| (width.round() as u32, height.round() as u32))
}

/// The output's pixel buffer attributes, all three mandatory (module doc): `'420f'`, IOSurface
/// backed (an empty properties dictionary asks for IOSurface backing with the defaults), Metal
/// compatible.
fn output_attributes() -> Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: framework constants of `CVPixelBuffer.h`, valid for the life of the process.
    let (format_key, surface_key, metal_key) = unsafe {
        (kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey)
    };
    let format = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
    let surface = NSDictionary::<NSString, AnyObject>::new();
    let metal = NSNumber::new_bool(true);
    NSDictionary::from_slices(
        &[bridged(format_key), bridged(surface_key), bridged(metal_key)],
        &[&*format as &AnyObject, &*surface as &AnyObject, &*metal as &AnyObject],
    )
}

/// A CoreFoundation string as the Foundation string it is toll free bridged to.
fn bridged(key: &'static CFString) -> &'static NSString {
    // SAFETY: `CFStringRef` and `NSString *` are toll free bridged: the same object, usable as
    // either type. The reference is a framework constant, so the `'static` lifetime holds.
    unsafe { &*(key as *const CFString).cast::<NSString>() }
}

/// Moves a buffer from the objc2 binding's type to gpui's `core_video` type.
///
/// Ownership: `copyPixelBufferForItemTime` follows the copy rule, so `buffer` owns one reference
/// and releases it when it drops at the end of this function. `wrap_under_get_rule` retains the
/// same buffer once more, so the returned value owns its own reference and releases it when it
/// drops. While both exist there are two owners; afterwards only the returned value.
fn rewrap(buffer: Retained<objc2_core_video::CVPixelBuffer>) -> CVPixelBuffer {
    let raw = Retained::as_ptr(&buffer) as CVPixelBufferRef;
    // SAFETY: `raw` is a live `CVPixelBufferRef` for the whole call, kept alive by `buffer`, and
    // it is a pixel buffer (the type the copy method returns), as the wrapper expects.
    unsafe { CVPixelBuffer::wrap_under_get_rule(raw) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile only: names the type so the module is in the test build and a drift from the
    /// `Player` trait fails the suite, without making an `AVPlayer` (tests use `FakePlayer`).
    #[test]
    fn the_player_module_builds_and_the_fake_is_still_the_only_one_tests_touch() {
        assert!(std::mem::size_of::<AvPlayer>() > 0);
    }
}
