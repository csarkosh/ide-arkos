//! Draws a video tab (spec 3.2 and 3.3): the picture (the player's frame through gpui's
//! `surface`, letterboxed on the window's colour), the control strip under it, and the reason
//! with a way out when the file cannot be played. The strip's arithmetic (`scrub_x`,
//! `scrub_time`, `strip_time`, `status_word`) is pure and tested; the rest reads the view and
//! the theme as they are at this draw, never keeping a colour.

use crate::theme::Theme;
use crate::video::{fmt_time, PlayerState, VideoModel};
use crate::video_view::VideoView;
use crate::Root;
use gpui::{
    canvas, div, img, point, prelude::*, px, size, surface, Bounds, ClickEvent, Context, DispatchPhase, Div, Entity, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, PathBuilder, Pixels, SharedString, Window,
};
use std::time::Instant;

/// The control strip's height, always shown (spec 3.3).
pub const STRIP_H: f32 = 28.0;
/// The scrubber's track, the loop span behind its played part, and the knob.
const TRACK_H: f32 = 4.0;
const LOOP_H: f32 = 8.0;
const KNOB: f32 = 10.0;
/// The play glyph's box.
const GLYPH: f32 = 12.0;

/// How far through `duration` `secs` is, from 0 to 1; 0 for a length that is not known, not
/// finite or not positive. Never `clamp`: a player value may be NaN.
fn fraction(secs: f64, duration: f64) -> f64 {
    if !(duration.is_finite() && duration > 0.0) {
        return 0.0;
    }
    let f = secs / duration;
    if f.is_nan() {
        0.0
    } else {
        f.max(0.0).min(1.0)
    }
}

/// The scrubber's x for a time on a track `width` wide: 0 at the start, `width` at the end.
pub fn scrub_x(secs: f64, duration: f64, width: f32) -> f32 {
    if !(width > 0.0) {
        return 0.0;
    }
    (fraction(secs, duration) * width as f64) as f32
}

/// The time under `x` on a track `width` wide: left of the track is the start, past it the end.
pub fn scrub_time(x: f32, duration: f64, width: f32) -> f64 {
    if !(width > 0.0) || !(duration.is_finite() && duration > 0.0) {
        return 0.0;
    }
    fraction(x as f64 / width as f64, 1.0) * duration
}

/// `position / duration` as the strip shows them, `m:ss.f` each and both with hours when the
/// file is an hour or longer; `--` for a length not known yet.
pub fn strip_time(position: f64, duration: Option<f64>) -> String {
    match duration {
        Some(d) if d >= 3600.0 && position < 3600.0 => {
            let short = fmt_time(position);
            let (m, rest) = short.split_once(':').unwrap_or(("0", "00.0"));
            format!("0:{m:0>2}:{rest} / {}", fmt_time(d))
        }
        Some(d) => format!("{} / {}", fmt_time(position), fmt_time(d)),
        None => format!("{} / --", fmt_time(position)),
    }
}

/// The word at the strip's right (spec 3.3): a notice while it lasts, else what the player is
/// doing, and nothing in the steady state.
pub fn status_word(model: &VideoModel, now: Instant) -> String {
    if let Some(notice) = model.notice_at(now) {
        return notice.to_string();
    }
    match &model.state {
        PlayerState::Ready => String::new(),
        PlayerState::Loading => "loading".into(),
        PlayerState::Reloading => "reloading".into(),
        PlayerState::Waiting => "waiting for file".into(),
        PlayerState::Unplayable(why) => why.clone(),
    }
}

/// A piece of the track from `from` to `to` (x within `b`), `h` tall, centred on its line.
fn span(b: Bounds<Pixels>, from: f32, to: f32, h: f32) -> Bounds<Pixels> {
    let mid = b.center().y;
    Bounds { origin: point(b.origin.x + px(from), mid - px(h / 2.0)), size: size(px((to - from).max(0.0)), px(h)) }
}

/// The play triangle, or the two bars of pause while it plays, in a `GLYPH` box.
fn paint_glyph(b: Bounds<Pixels>, playing: bool, color: Hsla, window: &mut Window) {
    let corner = point(b.center().x - px(GLYPH / 2.0), b.center().y - px(GLYPH / 2.0));
    let at = |x: f32, y: f32| point(corner.x + px(x), corner.y + px(y));
    if playing {
        for x in [2.0, 7.0] {
            let bar = Bounds { origin: at(x, 1.0), size: size(px(3.0), px(GLYPH - 2.0)) };
            window.paint_quad(gpui::fill(bar, color).corner_radii(px(1.0)));
        }
    } else {
        let mut p = PathBuilder::fill();
        p.move_to(at(2.0, 1.0));
        p.line_to(at(2.0, GLYPH - 1.0));
        p.line_to(at(GLYPH - 1.0, GLYPH / 2.0));
        p.close();
        if let Ok(path) = p.build() {
            window.paint_path(path, color);
        }
    }
}

/// The video tab's body: the picture over the control strip, holding the keys of the "Video"
/// context while focused. Every draw takes the newest frame; display frames are asked for only
/// while the view needs them (`VideoView::frame_step`), so a paused tab paints nothing.
pub fn render(view: &Entity<VideoView>, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Div {
    let now = Instant::now();
    let more = view.update(cx, |v, cx| v.frame_step(now, cx));
    let v = view.read(cx);
    let c = &theme.chrome;
    let (text, dim, bg) = (c.text.hsla(), c.text_dim.hsla(), c.window_bg.hsla());
    let eid = view.entity_id();
    let playing = v.model.playing;
    let (position, duration) = (v.position(), v.duration());
    let loop_range = v.model.loop_range;
    let selection = v.selection();
    let dragging = v.scrub.is_some_and(|s| s.dragging);
    let status = status_word(&v.model, now);
    let path = v.model.path.clone();
    let (focus, muted) = (v.focus.clone(), v.model.muted);
    let unplayable = match &v.model.state {
        PlayerState::Unplayable(why) => Some(why.clone()),
        _ => None,
    };
    let waiting = v.model.state == PlayerState::Waiting;
    let word = match v.model.state {
        PlayerState::Waiting => "waiting for file",
        PlayerState::Reloading => "reloading",
        PlayerState::Unplayable(_) => "",
        PlayerState::Loading | PlayerState::Ready => "loading",
    };
    let picture = match (&v.frame, &v.poster) {
        (Some(frame), _) => surface(frame.clone()).object_fit(ObjectFit::Contain).size_full().into_any_element(),
        (None, Some(poster)) => img(poster.clone()).object_fit(ObjectFit::Contain).size_full().into_any_element(),
        (None, None) => div().size_full().flex().items_center().justify_center().text_color(dim).child(word).into_any_element(),
    };
    // While the file is missing the last frame stays, dimmed to half, with the word over it.
    let dimmed = (waiting && (v.frame.is_some() || v.poster.is_some()))
        .then(|| div().absolute().inset_0().bg(bg.opacity(0.5)).flex().items_center().justify_center().text_color(text).child("waiting for file"));
    // A file that cannot be played: the reason and the way out the markdown poster has.
    let fallback = unplayable.map(|why| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .child(div().text_color(dim).child(why))
            .child(
                div()
                    .id(("video-quicktime", eid))
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(c.banner.hsla())
                    .border_1()
                    .border_color(c.border.hsla())
                    .text_color(text)
                    .cursor_pointer()
                    .child("Open in QuickTime Player")
                    .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| crate::md_view::open_media(path.clone(), crate::media::quicktime_args, cx))),
            )
    });
    let (border, span_bg) = (c.border.hsla(), c.focus_ring.hsla().opacity(0.4));
    // Every listener holds the view weakly: a closed tab's view (and its player) goes when `Root`
    // drops it, not when the next frame replaces these closures.
    let (track_view, drag_view) = (view.downgrade(), view.downgrade());
    let track = canvas(
        move |bounds, window, cx| {
            // One request per draw while the view needs frames (playing, loading, or a paused
            // seek's frame on its way); none otherwise.
            if more {
                window.request_animation_frame();
            }
            let _ = track_view.update(cx, |v, _| v.track = Some(bounds));
        },
        move |b, _, window, _cx| {
            let w = f32::from(b.size.width);
            let d = duration.unwrap_or(0.0);
            window.paint_quad(gpui::fill(span(b, 0.0, w, TRACK_H), border).corner_radii(px(TRACK_H / 2.0)));
            for (start, end) in loop_range.into_iter().chain(selection) {
                window.paint_quad(gpui::fill(span(b, scrub_x(start, d, w), scrub_x(end, d, w), LOOP_H), span_bg).corner_radii(px(2.0)));
            }
            let x = scrub_x(position, d, w);
            window.paint_quad(gpui::fill(span(b, 0.0, x, TRACK_H), text).corner_radii(px(TRACK_H / 2.0)));
            if duration.is_some() {
                let knob = Bounds { origin: point(b.origin.x + px(x - KNOB / 2.0), b.center().y - px(KNOB / 2.0)), size: size(px(KNOB), px(KNOB)) };
                window.paint_quad(gpui::fill(knob, text).corner_radii(px(KNOB / 2.0)));
            }
            // A drag follows the pointer anywhere in the window until the button comes up.
            if dragging {
                let moved = drag_view.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let held = ev.pressed_button == Some(MouseButton::Left);
                        let _ = moved.update(cx, |v, cx| v.scrub_move(f32::from(ev.position.x), held, cx));
                    }
                });
            }
            // The release is listened for on every paint, not only one made while dragging: a
            // trackpad tap's button comes up before the press has been painted, and a release
            // that is not a drag's changes nothing (`scrub_release`).
            let released = drag_view.clone();
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                    let _ = released.update(cx, |v, cx| v.scrub_release(cx));
                }
            });
        },
    )
    .size_full();
    let press = view.downgrade();
    let scrubber = div()
        .id(("video-scrubber", eid))
        .flex_1()
        .min_w(px(40.0))
        .h_full()
        // Half the knob either side, so it is whole at both ends.
        .px(px(KNOB / 2.0))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_root, ev: &MouseDownEvent, _window, cx| {
                let _ = press.update(cx, |v, cx| v.scrub_press(f32::from(ev.position.x), ev.modifiers.shift, cx));
            }),
        )
        .child(track);
    let toggle = |id: &'static str, label: &'static str, on: bool, act: fn(&mut VideoView, &mut Context<VideoView>), cx: &mut Context<Root>| {
        let target = view.downgrade();
        div()
            .id((id, eid))
            .flex_none()
            .text_color(if on { c.focus_ring.hsla() } else { dim })
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| {
                let _ = target.update(cx, |v, cx| act(v, cx));
            }))
    };
    let looping = toggle("video-loop", "loop", loop_range.is_some(), VideoView::toggle_loop, cx);
    let mute = toggle("video-mute", "mute", muted, VideoView::mute, cx);
    let play_target = view.downgrade();
    let play_button = div()
        .id(("video-play", eid))
        .flex_none()
        .size(px(GLYPH + 4.0))
        .cursor_pointer()
        .child(canvas(|_, _, _| (), move |b, _, window, _| paint_glyph(b, playing, text, window)).size_full())
        .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| {
            let _ = play_target.update(cx, |v, cx| v.play_pause(cx));
        }));
    let strip = div()
        .flex_none()
        .h(px(STRIP_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .bg(c.tab_bar.hsla())
        .text_color(text)
        .font_family(family)
        .text_size(px(12.0))
        .child(play_button)
        .child(div().flex_none().child(strip_time(position, duration)))
        .child(scrubber)
        .child(looping)
        .child(mute)
        .when(!status.is_empty(), |d| d.child(div().flex_none().max_w(px(280.0)).truncate().text_color(dim).child(status)));
    div()
        .size_full()
        .flex()
        .flex_col()
        .key_context("Video")
        .track_focus(&focus)
        .bg(bg)
        .child(div().flex_1().min_h_0().relative().overflow_hidden().child(picture).children(dimmed).children(fallback))
        .child(strip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::{PlayerState, VideoModel, NOTICE_FOR};
    use std::time::Instant;

    #[test]
    fn scrub_x_and_scrub_time_are_inverses_and_clamp() {
        let (d, w) = (60.0, 600.0);
        assert_eq!(scrub_x(0.0, d, w), 0.0);
        assert_eq!(scrub_x(d, d, w), w);
        assert_eq!(scrub_time(0.0, d, w), 0.0);
        assert_eq!(scrub_time(w, d, w), d);
        for secs in [0.0, 1.5, 12.25, 30.0, 59.9] {
            let x = scrub_x(secs, d, w);
            assert!((scrub_time(x, d, w) - secs).abs() < 1e-3, "{secs} -> {x}");
        }
        assert_eq!(scrub_time(-40.0, d, w), 0.0, "a press left of the track is the start");
        assert_eq!(scrub_time(w + 40.0, d, w), d, "a drag past the track is the end");
        assert_eq!(scrub_x(-3.0, d, w), 0.0);
        assert_eq!(scrub_x(99.0, d, w), w);
        // A duration the player does not know (or that is not a length) keeps the knob at the start.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(scrub_x(5.0, bad, w), 0.0, "{bad}");
            assert_eq!(scrub_time(300.0, bad, w), 0.0, "{bad}");
        }
        assert_eq!(scrub_x(5.0, d, 0.0), 0.0);
        assert_eq!(scrub_time(5.0, d, 0.0), 0.0);
        assert_eq!(scrub_time(f32::NAN, d, w), 0.0);
        assert_eq!(scrub_x(f64::NAN, d, w), 0.0);
    }

    #[test]
    fn the_strip_shows_hours_only_for_a_file_over_an_hour() {
        assert_eq!(strip_time(72.25, Some(130.0)), "1:12.2 / 2:10.0");
        assert_eq!(strip_time(72.25, Some(3725.0)), "0:01:12.2 / 1:02:05.0");
        assert_eq!(strip_time(3700.0, Some(3725.0)), "1:01:40.0 / 1:02:05.0");
        assert_eq!(strip_time(3.0, None), "0:03.0 / --", "a length not known yet");
    }

    #[test]
    fn the_status_word_is_quiet_when_ready_and_says_why_otherwise() {
        let now = Instant::now();
        let mut m = VideoModel::new("/p/x.mp4".into(), None, None);
        assert_eq!(status_word(&m, now), "loading");
        m.state = PlayerState::Ready;
        assert_eq!(status_word(&m, now), "", "nothing in the steady state");
        m.state = PlayerState::Reloading;
        assert_eq!(status_word(&m, now), "reloading");
        m.state = PlayerState::Waiting;
        assert_eq!(status_word(&m, now), "waiting for file");
        m.state = PlayerState::Unplayable("cannot play: The file is damaged.".into());
        assert_eq!(status_word(&m, now), "cannot play: The file is damaged.", "the reason, as the player gave it");
        m.state = PlayerState::Ready;
        m.notice = Some(("new cut".into(), now));
        assert_eq!(status_word(&m, now), "new cut");
        assert_eq!(status_word(&m, now + NOTICE_FOR), "", "a notice lasts NOTICE_FOR");
    }
}
