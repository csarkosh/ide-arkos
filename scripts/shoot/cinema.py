#!/usr/bin/env python3
"""The hero clip: a camera move over the composed windows, the way a product page's opening
video moves (store.google.com's Pixel pages, apple.com's), ending on the key art and holding.

The scene is the hero's three windows on a dark ground: the front one alive (a recording of it,
frame by frame), the two behind it stills, fanned a little higher like postcards. The camera
starts tight on the front window's tab bar, seen a little from the side, the backs soft and out
of focus, and pulls back and straightens over the move, the backs sharpening as it goes and
drifting less than the front (they are further away), until the whole key art is in frame; then
it holds. Cubic ease-out: fast to leave, slow to land.

    python3 -I scripts/shoot/cinema.py <out.mp4> --front <recording.mp4> --back <still.png> --back <still.png>
        [--width 2880] [--height 1600] [--fps 24] [--move 4.6] [--hold 0.9] [--from 0]
        [--poster out.jpg] [--preview]

`--front` is a recording of the front window as record.swift makes it (the window alone, no
shadow), read from `--from` seconds in; `--back` stills are framed captures (frame.py's output,
with their shadows). `--preview` renders at a quarter of the size and 15 fps, to look before
the real render. The last frame is written as `--poster`. Needs ffmpeg, Pillow and numpy."""
import argparse
import json
import math
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

# The key art, as fractions of the frame: where each window sits once the camera has landed.
FRONT_W = 0.50          # the front window's width
FRONT_TOP = 0.27        # its top edge; the ground above it is where the page lays its title
BACK_W = 0.35
BACKS = [              # (centre x, top y, tilt in degrees) for the two behind, left then right
    (0.20, 0.19, -3.0),
    (0.80, 0.21, 3.0),
]
GROUND = (9, 11, 16)
GLOW = (46, 62, 108)
HAZE = (22, 28, 44)
START_ZOOM = 3.2        # how tight the first frame is
START_AT = (0.27, 0.06)  # the point of the front window the first frame centres on (fractions of it)
START_KEYSTONE = 0.13   # how much the first frame is seen from the side
BACK_DEPTH = 0.72       # how much of the camera's move the backs follow (less: they are further)
BACK_BLUR = 0.014       # the backs' blur at the start, as a fraction of the frame's width
FRONT_BLUR = 0.004      # the front's, which resolves in the first moments (a rack of focus)
CORNER = 0.0095         # a window's corner radius, as a fraction of the front window's width
SHADOW = 0.03
REFLECTION = 0.22       # how much of the front window's height mirrors on the ground below it
REFLECTION_ALPHA = 0.28


def ease_out(t):
    """Slow to leave and slower to land: a cubic in and out, weighted to the landing."""
    t = min(1.0, max(0.0, t))
    return 4 * t ** 3 if t < 0.5 else 1 - (-2 * t + 2) ** 3 / 2


def homography(src, dst):
    """The 3x3 matrix taking the four `src` points to the four `dst` points."""
    rows = []
    for (x, y), (u, v) in zip(src, dst):
        rows.append([x, y, 1, 0, 0, 0, -u * x, -u * y])
        rows.append([0, 0, 0, x, y, 1, -v * x, -v * y])
    coeffs = np.linalg.solve(np.array(rows, dtype=float), np.array([c for p in dst for c in p], dtype=float))
    return np.array([*coeffs, 1.0]).reshape(3, 3)


def camera(t, depth, W, H, front_box):
    """The quad of the scene the output frame shows at `t` (0 to 1 along the move) for a layer
    at `depth` (1 the front, less behind): its corners in scene pixels, top-left first."""
    e = ease_out(t)
    k = 1 - (1 - e) * depth
    fx, fy, fw, fh = front_box
    start_c = np.array([fx + START_AT[0] * fw, fy + START_AT[1] * fh])
    end_c = np.array([W / 2, H / 2])
    c = end_c + (start_c - end_c) * (1 - k)
    zoom = 1 + (START_ZOOM - 1) * (1 - k)
    hw, hh = W / 2 / zoom, H / 2 / zoom
    ks = START_KEYSTONE * (1 - k)
    # Seen from the left: the left edge nearer (taller), the right edge further (shorter).
    return [
        (c[0] - hw, c[1] - hh * (1 + ks)),
        (c[0] + hw, c[1] - hh * (1 - ks)),
        (c[0] + hw, c[1] + hh * (1 - ks)),
        (c[0] - hw, c[1] + hh * (1 + ks)),
    ]


def place(layer_size, box, tilt=0.0):
    """Where a layer's corners land in the scene: its box (x, y, w, h), tilted about its centre."""
    x, y, w, h = box
    cx, cy = x + w / 2, y + h / 2
    a = math.radians(tilt)
    pts = []
    for px, py in [(0, 0), (w, 0), (w, h), (0, h)]:
        dx, dy = px - w / 2, py - h / 2
        pts.append((cx + dx * math.cos(a) - dy * math.sin(a), cy + dx * math.sin(a) + dy * math.cos(a)))
    return pts


def draw_layer(canvas, layer, layer_quad, cam_quad, W, H, blur=0.0):
    """`layer` (RGBA) whose corners sit at `layer_quad` in the scene, drawn onto `canvas` as the
    camera quad `cam_quad` sees it, blurred by `blur` pixels of the output when out of focus."""
    lw, lh = layer.size
    scene_to_layer = homography(layer_quad, [(0, 0), (lw, 0), (lw, lh), (0, lh)])
    out_to_scene = homography([(0, 0), (W, 0), (W, H), (0, H)], cam_quad)
    m = scene_to_layer @ out_to_scene
    m = m / m[2, 2]
    # A quick reject: the layer's corners in output space, all off one side means nothing to draw.
    inv = np.linalg.inv(m)
    corners = [inv @ np.array([x, y, 1.0]) for x, y in [(0, 0), (lw, 0), (lw, lh), (0, lh)]]
    xs = [c[0] / c[2] for c in corners]
    ys = [c[1] / c[2] for c in corners]
    if max(xs) < 0 or min(xs) > W or max(ys) < 0 or min(ys) > H:
        return
    warped = layer.transform((W, H), Image.Transform.PERSPECTIVE, tuple(m.flatten()[:8]), Image.Resampling.BICUBIC)
    if blur > 0.3:
        warped = warped.filter(ImageFilter.GaussianBlur(blur))
    canvas.alpha_composite(warped)


def shadow_for(size, radius, pad):
    """A window's soft shadow, as its own layer a little larger than the window."""
    w, h = size
    img = Image.new("RGBA", (w + 2 * pad, h + 2 * pad), (0, 0, 0, 0))
    ImageDraw.Draw(img).rounded_rectangle((pad, pad + pad // 3, pad + w, pad + h + pad // 3), radius=radius, fill=(0, 0, 0, 170))
    return img.filter(ImageFilter.GaussianBlur(pad / 2.2))


def ground(W, H):
    """The dark ground: a haze that lifts towards the top, and a pool of light where the front
    window lands, as a product sits in a lit studio."""
    g = Image.new("RGB", (W, H), GROUND)
    haze = Image.linear_gradient("L").resize((W, H)).transpose(Image.Transpose.FLIP_TOP_BOTTOM)
    g = Image.composite(Image.new("RGB", (W, H), HAZE), g, haze.point(lambda v: int(v * 0.7)))
    glow = Image.new("L", (W, H), 0)
    ImageDraw.Draw(glow).ellipse((W * 0.10, H * 0.45, W * 0.90, H * 1.30), fill=255)
    glow = glow.filter(ImageFilter.GaussianBlur(W * 0.09))
    g = Image.composite(Image.new("RGB", (W, H), GLOW), g, glow.point(lambda v: int(v * 0.9)))
    return g.convert("RGBA")


def reflection_of(frame, depth_px):
    """The front window mirrored on the ground under it, fading out over `depth_px`."""
    w = frame.width
    mirror = frame.transpose(Image.Transpose.FLIP_TOP_BOTTOM).crop((0, 0, w, depth_px))
    fade = Image.linear_gradient("L").resize((w, depth_px)).transpose(Image.Transpose.FLIP_TOP_BOTTOM).point(lambda v: int(v * REFLECTION_ALPHA))
    alpha = Image.composite(fade, Image.new("L", (w, depth_px), 0), mirror.getchannel("A"))
    mirror.putalpha(alpha)
    return mirror.filter(ImageFilter.GaussianBlur(max(1, w * 0.002)))


def frames_of(path, start, fps, count, size):
    """`count` frames of the recording from `start` seconds, at `fps`, scaled to `size`."""
    w, h = size
    p = subprocess.Popen([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-nostdin", "-ss", f"{start}", "-i", str(path),
        "-vf", f"fps={fps},scale={w}:{h}:flags=lanczos", "-frames:v", str(count), "-f", "rawvideo", "-pix_fmt", "rgba", "-",
    ], stdout=subprocess.PIPE)
    n = w * h * 4
    while True:
        buf = p.stdout.read(n)
        if len(buf) < n:
            break
        yield Image.frombytes("RGBA", (w, h), buf)
    p.wait()


def probe(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height:format=duration", "-of", "json", str(path)], capture_output=True, text=True, check=True).stdout
    j = json.loads(out)
    return int(j["streams"][0]["width"]), int(j["streams"][0]["height"]), float(j["format"]["duration"])


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("out")
    p.add_argument("--front", required=True, help="the front window's recording (record.swift's)")
    p.add_argument("--back", action="append", default=[], help="a framed still behind it (frame.py's); left then right")
    p.add_argument("--width", type=int, default=2880)
    p.add_argument("--height", type=int, default=1600)
    p.add_argument("--fps", type=int, default=24)
    p.add_argument("--move", type=float, default=4.6, help="seconds the camera moves")
    p.add_argument("--hold", type=float, default=0.9, help="seconds it holds on the key art")
    p.add_argument("--from", dest="start", type=float, default=0.0, help="seconds into the recording the clip starts")
    p.add_argument("--poster", help="write the last frame here as a JPEG")
    p.add_argument("--preview", action="store_true", help="a quarter of the size at 15 fps")
    a = p.parse_args()
    if len(a.back) != 2:
        sys.exit("cinema.py: two --back stills, left then right")
    W, H, fps = (a.width // 4, a.height // 4, 15) if a.preview else (a.width, a.height, a.fps)
    W, H = W & ~1, H & ~1
    rw, rh, dur = probe(a.front)
    total = a.move + a.hold
    if a.start + total > dur + 0.05:
        sys.exit(f"cinema.py: {a.front} is {dur:.1f} s; the clip needs {total:.1f} s from {a.start:.1f}")
    n = round(total * fps)

    # The key art's layout in scene pixels, which are the output frame's at the end of the move.
    fw = round(W * FRONT_W)
    fh = round(fw * rh / rw)
    front_box = (round((W - fw) / 2), round(H * FRONT_TOP), fw, fh)
    radius = max(2, round(fw * CORNER))
    pad = max(4, round(fw * SHADOW))
    mask = Image.new("L", (fw, fh), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, fw - 1, fh - 1), radius=radius, fill=255)
    front_shadow = shadow_for((fw, fh), radius, pad)
    front_shadow_quad = place(front_shadow.size, (front_box[0] - pad, front_box[1] - pad, fw + 2 * pad, fh + 2 * pad))
    front_quad = place((fw, fh), front_box)
    depth = round(fh * REFLECTION)
    mirror_quad = place((fw, depth), (front_box[0], front_box[1] + fh, fw, depth))

    backs = []
    for path, (cx, top, tilt) in zip(a.back, BACKS):
        still = Image.open(path).convert("RGBA")
        bw = round(W * BACK_W)
        bh = round(still.height * bw / still.width)
        still = still.resize((bw, bh), Image.Resampling.LANCZOS)
        backs.append((still, place((bw, bh), (round(cx * W - bw / 2), round(top * H), bw, bh), tilt)))

    bg = ground(W, H)
    enc = subprocess.Popen([
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(fps), "-i", "-",
        "-vf", "setparams=color_primaries=bt709:color_trc=iec61966-2-1:colorspace=bt709:range=tv,format=yuv420p",
        "-c:v", "libx264", "-preset", "slow" if not a.preview else "veryfast", "-crf", "19", "-profile:v", "high",
        "-movflags", "+faststart", "-an", a.out,
    ], stdin=subprocess.PIPE)
    last = None
    for i, frame in enumerate(frames_of(a.front, a.start, fps, n, (fw, fh))):
        t = min(1.0, (i / fps) / a.move) if a.move > 0 else 1.0
        canvas = bg.copy()
        cam_back = camera(t, BACK_DEPTH, W, H, front_box)
        blur = W * BACK_BLUR * (1 - ease_out(t))
        for still, quad in backs:
            draw_layer(canvas, still, quad, cam_back, W, H, blur)
        cam_front = camera(t, 1.0, W, H, front_box)
        frame.putalpha(mask)
        draw_layer(canvas, reflection_of(frame, depth), mirror_quad, cam_front, W, H)
        draw_layer(canvas, front_shadow, front_shadow_quad, cam_front, W, H)
        draw_layer(canvas, frame, front_quad, cam_front, W, H, W * FRONT_BLUR * max(0.0, 1 - t * 3))
        last = canvas.convert("RGB")
        enc.stdin.write(last.tobytes())
        if i % 30 == 0:
            print(f"cinema: frame {i} of {n}", file=sys.stderr)
    enc.stdin.close()
    enc.wait()
    if enc.returncode != 0:
        sys.exit("cinema.py: ffmpeg failed")
    if a.poster and last is not None:
        last.save(a.poster, "JPEG", quality=90, optimize=True, progressive=True)
    print(f"{a.out}: {W}x{H}, {n} frames at {fps} fps ({a.move:.1f} s move, {a.hold:.1f} s hold)")


if __name__ == "__main__":
    main()
