#!/usr/bin/env python3
"""A window recording (scripts/shoot/record.swift) made a seamless loop for a web page.

The recording shows the window through a storyboard's themes, each held the same time, and
then back on the first one: one period long (the themes' holds together), plus a tail on the
first theme again. The loop is the period, its seam hidden by cross-fading that tail (the
return to the first theme, as the window really drew it) into the opening seconds, so the last
frame leads into the first with no cut:

    python3 -I scripts/shoot/loop.py <in.mp4> <out.mp4> --period 16 [--seam 1] [--fps 30] [--width 1320] [--poster out.jpg]

The frames are re-timed at a constant rate (ScreenCaptureKit delivers one only when the window
changes), scaled to at most `width` across, and written as H.264 without sound, ready to
stream; `--poster` writes the first frame as a JPEG. Needs ffmpeg."""
import argparse
import json
import subprocess
import sys


def duration_of(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", path], capture_output=True, text=True, check=True).stdout
    return float(json.loads(out)["format"]["duration"])


def loop_filter(period, seam, fps, width):
    """The ffmpeg filter graph: the body (from the seam's length in to the period's end), then
    the tail (the first seam's length after the period) cross-faded into the head (the first
    seam's length), so the output is exactly one period long."""
    prep = f"fps={fps},scale='min(iw,{width})':-2:flags=lanczos,format=yuv420p,setpts=PTS-STARTPTS"
    return (
        f"[0:v]setpts=PTS-STARTPTS,{prep},split=3[a][b][c];"
        f"[a]trim=start={seam}:end={period},setpts=PTS-STARTPTS[body];"
        f"[b]trim=start={period}:end={period + seam},setpts=PTS-STARTPTS[tail];"
        f"[c]trim=start=0:end={seam},setpts=PTS-STARTPTS[head];"
        f"[tail][head]xfade=transition=fade:duration={seam}:offset=0[joint];"
        f"[body][joint]concat=n=2:v=1:a=0[out]"
    )


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("source")
    p.add_argument("out")
    p.add_argument("--period", type=float, required=True, help="the loop's length in seconds: the themes' holds together")
    p.add_argument("--seam", type=float, default=1.0, help="seconds of cross-fade from the end into the start")
    p.add_argument("--fps", type=int, default=30)
    p.add_argument("--width", type=int, default=1320, help="the widest the picture is scaled to, in pixels")
    p.add_argument("--poster", help="write the first frame here as a JPEG")
    a = p.parse_args()
    if not 0.1 <= a.seam < a.period:
        sys.exit(f"loop.py: the seam ({a.seam} s) must be shorter than the period ({a.period} s)")
    have = duration_of(a.source)
    need = a.period + a.seam
    if have + 0.05 < need:
        sys.exit(f"loop.py: {a.source} is {have:.2f} s, shorter than the period and seam together ({need:.2f} s)")
    subprocess.run([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", a.source,
        "-filter_complex", loop_filter(a.period, a.seam, a.fps, a.width), "-map", "[out]",
        "-c:v", "libx264", "-preset", "slow", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an", a.out,
    ], check=True)
    if a.poster:
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", a.out, "-frames:v", "1", "-q:v", "3", a.poster], check=True)
    made = duration_of(a.out)
    print(f"{a.out}: {made:.2f} s loop from {have:.2f} s recorded")


if __name__ == "__main__":
    main()
