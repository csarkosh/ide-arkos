# The update asks first, and shows its progress

Cyrus, 2026-11-16, after the update veil (0.8.42): still jarring. His answers, the same night:
it starts without warning, the blur itself, and it is too fast. Then: "Leave it open until the
user either clicks update or later. open the toast regardless of user activity. when the user
has a pending update, the notification button remains with its notification dot until the user
updates. there should be a button in that menu too for updating the IDE." And: "I want actual
progress indication with metrics visible and updating as the update runs. I do not want an
indefinite loaders."

## 1. The toast

As soon as an update is ready (a release staged, or the build on disk newer after another
window applied it), a toast at the window's bottom right says "Arkos IDE X is ready" (or "A new
version of Arkos IDE is ready"), "Your sessions keep running through it.", with Later and
Update. It stays until one is clicked, whatever the user does. Later puts it away for an hour,
then it shows again. The bell keeps its dot and its "Update ready" row with Update now all the
while; the toast's Update and the bell's go the same way (`update_clicked`).

A window that is not the app in front, idle by the old rule (thirty seconds, nothing open),
updates by itself: nobody watches it (`idle::update_plan`).

## 2. The card

The full-window frost becomes a light dim (22 % black) with a small card in the middle:
"Updating Arkos IDE", a bar that fills, and a line under it with the step, its count and the
seconds since the update began (`update_veil::Step`): Installing X (a fifth), Starting the new
window (two fifths), Restoring sessions N of M (to 95 %, a session at a time), Done. The old
window writes when it began to `update-started` in the app's folder, so the new window's clock
goes on from it. The new window stays hidden until its sessions are back, so its card shows the
count complete, then Done, then lifts.

Timing: in over 500 ms (long2, decelerate), held 600 ms before the old window asks for its
successor, held 700 ms on the new window (the count, then Done), out over 450 ms (long1,
accelerate). Reduce Motion: at once.

## 3. Previewing

`ARKOS_UPDATE_PREVIEW=<secs>` makes a sandbox window act as if a release were staged: the toast,
the bell's row, and on Update a short Installing step, then the real relaunch through the
daemon. A window an update started ignores it, so it runs once per launch.

## 4. Tested

Headless: the plan (behind and quiet, in front, Later), the bar's fill never going back, the
line's words, the start note read back and refused when stale, the toast's words. On screen (a
person, the Status table): the preview above.
