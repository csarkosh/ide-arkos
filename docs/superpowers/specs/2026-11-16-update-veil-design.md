# The update's veil

Cyrus, 2026-11-16, after the handoff (0.8.40): "the updating experience is better than before,
but is still jarring of how sudden it is. Is it possible to add a smooth, full-screen effect blur
with an "Automatic update applying..." or something similar to that so the UX is less jarring to
the user?"

## 1. What the user sees

1. The update applies (the user idle thirty seconds, or "Update now"): a blur comes over the
   whole window in 300 ms (Material medium2, standard decelerate), with "Updating Arkos IDE" and,
   under it, "Your sessions keep running." in the middle. The traffic lights stay on top.
2. Behind it the window hands off (seamless update handoff spec) or relaunches the old way.
3. The new window starts under the same blur at full strength. A handoff's successor lifts it
   200 ms after it showed itself (the old window has left from under it by then); a window an
   update relaunched without a handoff lifts it once the terminals of the page shown are back,
   or after 4 s. It goes in 250 ms (medium1, standard accelerate).

Under Reduce Motion it comes and goes at once. A relaunch the daemon refuses, or one with no
binary to relaunch, lifts the blur and says why, as before.

## 2. How

AppKit's own blur, an `NSVisualEffectView` blending within the window, laid over gpui's view as
a web tab's view is (`update_veil.rs`); its opacity is stepped from `motion`'s curves, so gpui
draws nothing for it and the terminals under it keep running. A window starts veiled when it is a
handoff's successor (`ARKOS_HANDOFF`) or says "Updated to X" (the version changed since the last
launch); a window the user or the watchdog opens does not.

Checked on screen (a throwaway local shoot, 2026-11-16): the blur covers gpui's Metal drawing,
the text is centred, the traffic lights show.

## 3. Tested

Headless: the durations are the tokens, the curve's ends, Reduce Motion, and which windows start
veiled. On screen (a person): the sequence of section 1 on a real update.
