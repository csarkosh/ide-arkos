#!/usr/bin/env python3
"""A window recording (scripts/shoot/record.swift) cut into a seamless loop for a web page.

The recording shows the window through a storyboard's themes, each held `hold` seconds, the
switches made through the IDE at each hold's start. The loop takes each theme's steady stretch,
from `settle` seconds after its switch (past the config poll and the IDE's own fade) to a tenth
of a second before the next, and joins the stretches with `fade` seconds of a transition: a
soft-edged sweep from left to right (`sweep`), the new theme washing over the window the way
light moves over a product in Apple's videos, or a plain cross-fade (`dissolve`). The seam is
the same transition from the last theme back into the first: the first stretch is used twice,
the output cut half a second past the transitions' edges, at the same frame of both uses, so
the last frame leads into the first with no cut.

    python3 -I scripts/shoot/loop.py <in.mp4> <out.mp4> --hold 6 --themes 4 [--settle 1] [--fade 1.2]
        [--transition sweep|dissolve] [--fps 30] [--width 2000] [--poster out.jpg]

The frames are re-timed at a constant rate (ScreenCaptureKit delivers one only when the window
changes), scaled to at most `width` across, and written as H.264 without sound, tagged sRGB and
BT.709 as Apple's are, ready to stream; `--poster` writes the first frame as a JPEG. Needs ffmpeg."""
import argparse
import json
import subprocess
import sys

GUARD = 0.1
# How far past a transition's end the cut is made, so the loop's first and last frames are both
# a theme alone: a soft sweep's trailing edge is still a band of the old theme at its last tick.
MARGIN = 0.5
TRANSITIONS = {"sweep": "smoothright", "dissolve": "fade"}


def duration_of(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", path], capture_output=True, text=True, check=True).stdout
    return float(json.loads(out)["format"]["duration"])


def stretches(hold, themes, settle):
    """Each theme's steady stretch in the recording, (start, end) seconds, and its length."""
    length = hold - settle - GUARD
    return [(hold * k + settle, hold * (k + 1) - GUARD) for k in range(themes)], length


def loop_filter(hold, themes, settle, fade, transition, fps, width):
    """The ffmpeg filter graph. The stretches are t0 to t(n-1), then t0 again as tn (long enough
    to carry the seam and `fade` more); each is swept into the next at the point where the one
    before has `fade` seconds left; the chain is cut to start `fade` into t0 and end `fade` into
    tn (both half a second clear of the transitions' edges), which is the same frame."""
    spans, length = stretches(hold, themes, settle)
    n = themes
    # The colour tags go on the frames (setparams), which is where libx264 reads them from.
    prep = f"fps={fps},scale='min(iw,{width})':-2:flags=lanczos:out_color_matrix=bt709:out_range=tv,setparams=color_primaries=bt709:color_trc=iec61966-2-1:colorspace=bt709:range=tv,format=yuv420p,setpts=PTS-STARTPTS"
    parts = [f"[0:v]{prep},split={n + 1}" + "".join(f"[s{k}]" for k in range(n + 1))]
    for k, (a, b) in enumerate(spans):
        parts.append(f"[s{k}]trim=start={a}:end={b},setpts=PTS-STARTPTS[t{k}]")
    a0 = spans[0][0]
    parts.append(f"[s{n}]trim=start={a0}:end={a0 + fade + MARGIN + GUARD},setpts=PTS-STARTPTS[t{n}]")
    kind = TRANSITIONS[transition]
    prev = "t0"
    for k in range(1, n + 1):
        offset = k * (length - fade)
        parts.append(f"[{prev}][t{k}]xfade=transition={kind}:duration={fade}:offset={offset:.4f}[x{k}]")
        prev = f"x{k}"
    cut = fade + MARGIN
    end = n * (length - fade) + cut
    parts.append(f"[{prev}]trim=start={cut}:end={end:.4f},setpts=PTS-STARTPTS[out]")
    return ";".join(parts), n * (length - fade)


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("source")
    p.add_argument("out")
    p.add_argument("--hold", type=float, required=True, help="seconds each theme was held")
    p.add_argument("--themes", type=int, required=True, help="how many themes were shown, the first again at the end not counted")
    p.add_argument("--settle", type=float, default=1.0, help="seconds after a switch before its theme counts as steady")
    p.add_argument("--fade", type=float, default=1.2, help="seconds a transition takes")
    p.add_argument("--transition", choices=sorted(TRANSITIONS), default="sweep")
    p.add_argument("--fps", type=int, default=30)
    p.add_argument("--width", type=int, default=2000, help="the widest the picture is scaled to, in pixels")
    p.add_argument("--poster", help="write the first frame here as a JPEG")
    p.add_argument("--plan", action="store_true", help="print the filter graph and the loop's length, and stop")
    a = p.parse_args()
    if a.themes < 2:
        sys.exit("loop.py: at least two themes")
    _, length = stretches(a.hold, a.themes, a.settle)
    if length < a.fade + 1.0:
        sys.exit(f"loop.py: a hold of {a.hold} s leaves {length:.2f} s steady, less than the fade ({a.fade} s) and a second of rest")
    if length < a.fade + MARGIN + GUARD:
        sys.exit(f"loop.py: a hold of {a.hold} s leaves {length:.2f} s steady, too little to cut the seam clear of its sweep")
    graph, made = loop_filter(a.hold, a.themes, a.settle, a.fade, a.transition, a.fps, a.width)
    if a.plan:
        print(graph.replace(";", ";\n"))
        print(f"loop: {made:.2f} s from {a.themes} themes")
        return
    have = duration_of(a.source)
    need = a.hold * a.themes
    if have + GUARD < need:
        sys.exit(f"loop.py: {a.source} is {have:.2f} s, shorter than the holds together ({need:.2f} s)")
    subprocess.run([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", a.source,
        "-filter_complex", graph, "-map", "[out]",
        "-c:v", "libx264", "-preset", "slow", "-crf", "19", "-profile:v", "high", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart", "-an", a.out,
    ], check=True)
    if a.poster:
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", a.out, "-frames:v", "1", "-q:v", "2", a.poster], check=True)
    got = duration_of(a.out)
    print(f"{a.out}: {got:.2f} s loop ({a.transition}, {a.fade} s each) from {have:.2f} s recorded")


if __name__ == "__main__":
    main()
