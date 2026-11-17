# The wash paced on the display's refreshes

Cyrus, 2026-11-17: Pane 0's graphics "look good on the native Macbook display, but jitters
significantly and loses its smoothness on non-native displays with alternative refresh rates. The
Pane 0 graphic should adapt to various refresh rates so it always appear smooth and without
jitters."

## Why it jittered

The wash was drawn by a sleep timer at the effect's cadence (30 a second at rest, 60 while a
highlight moves), and the shader's clock was read whenever the timer woke. A fixed-refresh display
shows a frame at its next refresh, so frames timed apart from the refreshes land on them unevenly
(30 a second on 75 Hz alternates two and three refreshes) and the timer's own wake-up jitter moves
the clock by more than the picture moves. ProMotion's variable refresh shows each frame as it
comes, which hid both on the MacBook.

## What draws it now

- gpui's display link, which runs on the refreshes of the display the window is on and moves with
  it to another screen, calls the window back on each refresh (`window.on_next_frame`). While the
  window is seen and the wash runs, `Root::start_wash_refreshes` keeps a chain of those callbacks
  going (`wash_refresh`, `Root::on_wash_refresh`); each asks for the next until the wash rests or
  stops.
- `effects::active::Pacer` learns the display's period: the median, over the latest refreshes, of
  the interval across eight of them (a late callback and an early one cancel; a missed one is an
  outlier), snapped to the screen's own rate (`NSScreen.maximumFramesPerSecond`, or a whole number
  of its refreshes) when they agree within a tenth.
- It draws on every k-th refresh, k the whole number nearest the cadence (the faster on a tie, held
  until the cadence is clearly nearer another): 30 a second is every 2nd refresh at 60 Hz and 75 Hz,
  4th at 120 Hz, 5th at 144 Hz.
- The time each frame is drawn for moves on by exactly the refreshes since the last, eased toward
  the clock by at most 50 microseconds a frame, so every frame moves the picture by the same step;
  it is set to the clock only when off by more than one and a half refreshes.
- The wash's timer loop keeps its other work: it rests the wash while the window cannot be seen,
  stops it under Reduce Motion or while paused, and looks in every tenth of a second, taking the
  wash back when the refreshes stop coming (the window covered, its display link stopped).

Tests (`effects::active`): every k-th refresh and even steps at 50, 60, 75, 100, 120 and 144 Hz
with callback jitter; a stall stays on the refresh grid; a move to a 144 Hz display is learned; a
busy main thread's callbacks (up to 10 ms late, one in twenty dropped) still learn 60 Hz and keep
four frames in five exactly two refreshes apart.
