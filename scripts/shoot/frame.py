#!/usr/bin/env python3
"""The raw window capture (scripts/shoot) framed for a page: the window's rounded corners cut
out, a soft shadow under it, on a transparent background, at the capture's 2x scale. Any
`--crop name:x,y,w,h` (fractions of the window) is also written beside the output as
<output stem>-<name>.png, unframed, with small rounded corners. `--overlay file:x,y` lays
another capture (the Chrome window Arkos docked) over the window at those pixels first.

    python3 -I scripts/shoot/frame.py <raw.png> <out.png> [--overlay chrome.png:1810,70] [--crop tabs:0,0,0.6,0.12]...

`compose` lays framed stills out as the hero: the first large in front, the others fanned
behind it, a caption under each (the scene's words), on a transparent ground:

    python3 -I scripts/shoot/frame.py compose <out.png> <front.png>:"The multi tasker" <back.png>:"The web developer" [<back2.png>:"The blogger"]

Needs Pillow."""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

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


CAPTION_FONT = "/System/Library/Fonts/Menlo.ttc"
# Sized for the README: GitHub shows the picture about 830 px wide, a third of its 2520 px
# canvas, so a 68 px caption reads at about 22 px there, and the longest ("The web developer")
# still fits the third of the width under its still.
CAPTION_SIZE = 34 * SCALE
CAPTION_PAD = (24 * SCALE, 11 * SCALE)
CANVAS_W = 1260 * SCALE
OUT_W = 1800


def caption_pill(text):
    """A dark pill with light text, readable on light and dark pages alike."""
    font = ImageFont.truetype(CAPTION_FONT, CAPTION_SIZE)
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    box = probe.textbbox((0, 0), text, font=font)
    tw, th = box[2] - box[0], box[3] - box[1]
    pw, ph = tw + 2 * CAPTION_PAD[0], th + 2 * CAPTION_PAD[1]
    pill = Image.new("RGBA", (pw, ph), (0, 0, 0, 0))
    draw = ImageDraw.Draw(pill)
    draw.rounded_rectangle((0, 0, pw - 1, ph - 1), radius=ph // 2, fill=(17, 19, 24, 240), outline=(232, 234, 240, 70), width=SCALE)
    draw.text((CAPTION_PAD[0] - box[0], CAPTION_PAD[1] - box[1]), text, font=font, fill=(232, 234, 240, 255))
    return pill


def compose(out, pieces):
    """pieces: [(path, caption)], the first in front. The backs stand half behind the front
    one, a little higher and tilted, as Day Hike's postcards do; each caption sits in a band
    under the stills, centred on the part of its still that shows, large enough to read at the
    size a README shows the picture."""
    W = CANVAS_W
    front = Image.open(pieces[0][0]).convert("RGBA")
    front_w = round(W * 0.58)
    front = front.resize((front_w, round(front.height * front_w / front.width)), Image.Resampling.LANCZOS)
    back_w = round(W * 0.44)
    backs = []
    for i, (path, _) in enumerate(pieces[1:]):
        b = Image.open(path).convert("RGBA")
        b = b.resize((back_w, round(b.height * back_w / b.width)), Image.Resampling.LANCZOS)
        b = b.rotate(2.5 if i % 2 == 0 else -2.5, expand=True, resample=Image.Resampling.BICUBIC)
        backs.append(b)
    front_x = (W - front_w) // 2
    front_y = round(front.height * 0.08)
    stills_bottom = max([front_y + front.height] + [b.height for b in backs])
    pills = [caption_pill(c) for _, c in pieces]
    band_y = stills_bottom + 6 * SCALE
    height = band_y + max(p.height for p in pills) + 4 * SCALE
    canvas = Image.new("RGBA", (W, height), (0, 0, 0, 0))
    centres = [W // 2]
    for i, b in enumerate(backs):
        x = 0 if i % 2 == 0 else W - b.width
        canvas.alpha_composite(b, (x, 0))
        # The part of the back the front leaves showing.
        centres.append(front_x // 2 if i % 2 == 0 else (front_x + front_w + W) // 2)
    canvas.alpha_composite(front, (front_x, front_y))
    for pill, cx in zip(pills, centres):
        x = min(max(cx - pill.width // 2, 0), W - pill.width)
        canvas.alpha_composite(pill, (x, band_y))
    canvas = canvas.crop(canvas.getbbox())
    canvas = canvas.resize((OUT_W, round(canvas.height * OUT_W / canvas.width)), Image.Resampling.LANCZOS)
    canvas.save(out, optimize=True)
    print(f"{out}: {canvas.width}x{canvas.height}")


def main(argv):
    if argv and argv[0] == "compose":
        if len(argv) < 4:
            sys.exit(__doc__)
        pieces = []
        for arg in argv[2:]:
            path, _, caption = arg.rpartition(":")
            pieces.append((Path(path), caption))
        return compose(Path(argv[1]), pieces)
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
