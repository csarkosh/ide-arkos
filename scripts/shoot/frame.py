#!/usr/bin/env python3
"""The raw window capture (scripts/shoot) framed for a page: the window's rounded corners cut
out, a soft shadow under it, on a transparent background, at the capture's 2x scale. Any
`--crop name:x,y,w,h` (fractions of the window) is also written beside the output as
<output stem>-<name>.png, unframed, with small rounded corners. `--overlay file:x,y` lays
another capture (the Chrome window Arkos docked) over the window at those pixels first.

    python3 -I scripts/shoot/frame.py <raw.png> <out.png> [--overlay chrome.png:1810,70] [--crop tabs:0,0,0.6,0.12]...

Needs Pillow."""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SCALE = 2
RADIUS = 12 * SCALE  # macOS windows' corner radius, about 12 points
PAD = 56 * SCALE
SHADOW_BLUR = 22 * SCALE
SHADOW_DROP = 12 * SCALE
SHADOW_ALPHA = 150
CROP_RADIUS = 6 * SCALE


def rounded(image, radius):
    mask = Image.new("L", image.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, image.width - 1, image.height - 1), radius=radius, fill=255)
    out = image.convert("RGBA")
    out.putalpha(mask)
    return out


def framed(window):
    w, h = window.size
    canvas = Image.new("RGBA", (w + 2 * PAD, h + 2 * PAD), (0, 0, 0, 0))
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((PAD, PAD + SHADOW_DROP, PAD + w, PAD + h + SHADOW_DROP), radius=RADIUS, fill=(0, 0, 0, SHADOW_ALPHA))
    shadow = shadow.filter(ImageFilter.GaussianBlur(SHADOW_BLUR))
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(rounded(window, RADIUS), (PAD, PAD))
    return canvas.crop(canvas.getbbox())


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    raw, out = Path(argv[0]), Path(argv[1])
    crops = []
    overlays = []
    args = argv[2:]
    while args:
        if len(args) < 2:
            sys.exit(f"unexpected argument {args[0]}")
        if args[0] == "--crop":
            name, _, rect = args[1].partition(":")
            x, y, w, h = (float(v) for v in rect.split(","))
            crops.append((name, x, y, w, h))
        elif args[0] == "--overlay":
            path, _, at = args[1].rpartition(":")
            x, y = (int(float(v)) for v in at.split(","))
            overlays.append((Path(path), x, y))
        else:
            sys.exit(f"unexpected argument {args[0]}")
        args = args[2:]
    window = Image.open(raw).convert("RGBA")
    for path, x, y in overlays:
        window.alpha_composite(Image.open(path).convert("RGBA"), (x, y))
    if window.getextrema()[3][0] == window.getextrema()[3][1] and len(set(window.convert("RGB").getcolors(16) or [1, 2])) <= 1:
        sys.exit(f"{raw} is one flat colour: the capture saw no window (Screen Recording permission?)")
    framed(window).save(out, optimize=True)
    print(f"{out}: {framed(window).size[0]}x{framed(window).size[1]}")
    for name, x, y, w, h in crops:
        box = (round(x * window.width), round(y * window.height), round((x + w) * window.width), round((y + h) * window.height))
        piece = rounded(window.crop(box), CROP_RADIUS)
        dest = out.with_name(f"{out.stem}-{name}.png")
        piece.save(dest, optimize=True)
        print(f"{dest}: {piece.width}x{piece.height}")


if __name__ == "__main__":
    main(sys.argv[1:])
