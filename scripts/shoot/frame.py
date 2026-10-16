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
CAPTION_SIZE = 15 * SCALE
CAPTION_PAD = (14 * SCALE, 7 * SCALE)
CAPTION_GAP = 10 * SCALE


def captioned(still, text, align):
    """The still with its caption on its bottom edge: a dark pill with light text, readable on
    light and dark pages alike, hung on the window's lower border at the left, centre or right
    (the side the still shows from behind the front one)."""
    font = ImageFont.truetype(CAPTION_FONT, CAPTION_SIZE)
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    box = probe.textbbox((0, 0), text, font=font)
    tw, th = box[2] - box[0], box[3] - box[1]
    pw, ph = tw + 2 * CAPTION_PAD[0], th + 2 * CAPTION_PAD[1]
    inset = 36 * SCALE
    px = {"left": PAD + inset, "center": (still.width - pw) // 2, "right": still.width - PAD - inset - pw}[align]
    py = still.height - PAD - ph // 2
    out = still.copy()
    pill = Image.new("RGBA", out.size, (0, 0, 0, 0))
    ImageDraw.Draw(pill).rounded_rectangle((px, py, px + pw, py + ph), radius=ph // 2, fill=(17, 19, 24, 240), outline=(232, 234, 240, 70), width=SCALE)
    out.alpha_composite(pill)
    ImageDraw.Draw(out).text((px + CAPTION_PAD[0] - box[0], py + CAPTION_PAD[1] - box[1]), text, font=font, fill=(232, 234, 240, 255))
    return out


def compose(out, pieces):
    """pieces: [(path, caption)], the first in front. The backs are scaled down and tilted a
    little, set behind and to the sides, higher than the front; the front sits lower and
    centred, as Day Hike's postcards do, and each still carries its caption where it shows."""
    front_w = 1520 * SCALE // 2
    back_w = round(front_w * 0.72)
    front = captioned(Image.open(pieces[0][0]).convert("RGBA"), pieces[0][1], "center")
    front = front.resize((front_w, round(front.height * front_w / front.width)), Image.Resampling.LANCZOS)
    placed = []
    for i, (path, caption) in enumerate(pieces[1:]):
        side = "left" if i % 2 == 0 else "right"
        b = captioned(Image.open(path).convert("RGBA"), caption, side)
        b = b.resize((back_w, round(b.height * back_w / b.width)), Image.Resampling.LANCZOS)
        b = b.rotate(3.0 if side == "left" else -3.0, expand=True, resample=Image.Resampling.BICUBIC)
        placed.append((side, b))
    # The backs show by almost half their width beside the front and rise above it.
    peek = round(back_w * 0.46)
    rise = round(front.height * 0.14)
    width = front.width + 2 * peek
    height = max([rise + front.height] + [b.height for _, b in placed]) + 4 * SCALE
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    for side, b in placed:
        canvas.alpha_composite(b, (0 if side == "left" else width - b.width, 0))
    canvas.alpha_composite(front, ((width - front.width) // 2, rise))
    canvas = canvas.crop(canvas.getbbox())
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
