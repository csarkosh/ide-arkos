#!/usr/bin/env python3
"""The raw window capture (scripts/shoot) framed for a page: the window's rounded corners cut
out, a soft shadow under it, on a transparent background, at the capture's 2x scale. Any
`--crop name:x,y,w,h` (fractions of the window) is also written beside the output as
<output stem>-<name>.png, unframed, with small rounded corners. `--overlay file:x,y` lays
another capture over the window at those pixels first.

    python3 -I scripts/shoot/frame.py <raw.png> <out.png> [--overlay chrome.png:1810,70] [--crop tabs:0,0,0.6,0.12]...

`compose` lays framed stills out as the hero: the first large in front, the others fanned
behind it, a caption on each (the scene's words), on a transparent ground; `compose-portrait`
the same as a tall picture, for a place beside text:

    python3 -I scripts/shoot/frame.py compose <out.png> <front.png>:"The multi tasker" <back.png>:"The web developer" [<back.png>:"The blogger" ...]

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
# Sized for the README: GitHub shows the picture about 830 px wide, a third of its canvas, so
# a 46 px caption reads at about 15 px there, and the longest ("The web developer") fits the
# part of its still the front one leaves showing.
CAPTION_SIZE = 23 * SCALE
CAPTION_PAD = (16 * SCALE, 8 * SCALE)
CANVAS_W = 1260 * SCALE
OUT_W = 1800


def captioned(still, text, align, scale, edge="bottom"):
    """The still with its caption hung on its lower (or upper) border at the left, centre or
    right (the side it shows from behind the front one): a dark pill with light text, readable
    on light and dark pages alike. `scale` is the still's size against the framed original,
    whose shadow padding the pill sits inside of."""
    font = ImageFont.truetype(CAPTION_FONT, CAPTION_SIZE)
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    box = probe.textbbox((0, 0), text, font=font)
    tw, th = box[2] - box[0], box[3] - box[1]
    pw, ph = tw + 2 * CAPTION_PAD[0], th + 2 * CAPTION_PAD[1]
    pad = round(PAD * scale)
    inset = 10 * SCALE
    px = {"left": pad + inset, "center": (still.width - pw) // 2, "right": still.width - pad - inset - pw}[align]
    # A top caption stays inside the still, whose shadow padding may be less than half the pill.
    py = still.height - pad - ph // 2 if edge == "bottom" else max(0, pad - ph // 2)
    out = still.copy()
    pill = Image.new("RGBA", out.size, (0, 0, 0, 0))
    ImageDraw.Draw(pill).rounded_rectangle((px, py, px + pw, py + ph), radius=ph // 2, fill=(17, 19, 24, 240), outline=(232, 234, 240, 70), width=SCALE)
    out.alpha_composite(pill)
    ImageDraw.Draw(out).text((px + CAPTION_PAD[0] - box[0], py + CAPTION_PAD[1] - box[1]), text, font=font, fill=(232, 234, 240, 255))
    return out


def compose(out, pieces):
    """pieces: [(path, caption)], the first in front. The backs stand behind the front one,
    a little higher and tilted, as Day Hike's postcards do: in turn left and right, the first
    pair against the edges with its captions on their bottom edges, each pair after it tucked
    in, higher and smaller with its captions on their top edges (the part that shows), and an
    odd last one at the top in the middle. The front sits lower and centred. One or two backs
    lie as the three-window hero always had them."""
    W = CANVAS_W
    front_w = round(W * 0.46)
    src = Image.open(pieces[0][0]).convert("RGBA")
    front = src.resize((front_w, round(src.height * front_w / src.width)), Image.Resampling.LANCZOS)
    front = captioned(front, pieces[0][1], "center", front_w / src.width)
    rest = pieces[1:]
    n = len(rest)
    pairs = n // 2
    deep = (pairs - 1) + (1 if n % 2 and n > 1 else 0)
    # (still, x, y, depth): the outer pair's top sits this far down for each tier behind it.
    lift = round(W * 0.075)
    placed = []
    back_h = None
    for i, (path, caption) in enumerate(rest):
        middle = n % 2 == 1 and i == n - 1 and n > 1
        j = pairs if middle else i // 2
        side = "center" if middle else ("left" if i % 2 == 0 else "right")
        width = round(W * (0.38 - 0.05 * j))
        src = Image.open(path).convert("RGBA")
        # The outer pair shares its first still's height, so a narrower still (the blogger's
        # stacked panes) is drawn narrower rather than taller, and never wider than the tier's
        # width, so a wide one does not outgrow the others.
        bw = width if j or back_h is None else min(width, round(src.width * back_h / src.height))
        b = src.resize((bw, round(src.height * bw / src.width)), Image.Resampling.LANCZOS)
        if j == 0:
            back_h = back_h or b.height
        b = captioned(b, caption, side, bw / src.width, edge="bottom" if j == 0 else "top")
        tilt = 0.0 if middle else (2.5 - 0.8 * j) * (1 if side == "left" else -1)
        b = b.rotate(tilt, expand=True, resample=Image.Resampling.BICUBIC)
        y = (deep - j) * lift if j <= deep else 0
        if middle:
            x = (W - b.width) // 2
        else:
            inset = round(W * 0.10 * j)
            x = inset if side == "left" else W - inset - b.width
        placed.append((b, x, y, j))
    base = deep * lift if n > 2 else 0
    front_y = base + round(front.height * 0.10)
    height = max([front_y + front.height] + [y + b.height for b, _, y, _ in placed]) + 4 * SCALE
    canvas = Image.new("RGBA", (W, height), (0, 0, 0, 0))
    for b, x, y, _ in sorted(placed, key=lambda p: -p[3]):
        canvas.alpha_composite(b, (x, y))
    canvas.alpha_composite(front, ((W - front_w) // 2, front_y))
    canvas = canvas.crop(canvas.getbbox())
    canvas = canvas.resize((OUT_W, round(canvas.height * OUT_W / canvas.width)), Image.Resampling.LANCZOS)
    canvas.save(out, optimize=True)
    print(f"{out}: {canvas.width}x{canvas.height}")


PORTRAIT_W = 600 * SCALE
PORTRAIT_OUT_W = 1000


def compose_portrait(out, pieces):
    """The same stills as a tall picture, for a place beside text (csarko.sh's card): the
    first behind-still at the top, the front one over its lower half, the second behind-still
    under the front one's lower half, tilted the other way; captions on the edges that show."""
    if len(pieces) > 3:
        return compose_portrait_fan(out, pieces)
    W = PORTRAIT_W
    front_w = round(W * 0.88)
    back_w = round(W * 0.78)
    src = Image.open(pieces[0][0]).convert("RGBA")
    front = src.resize((front_w, round(src.height * front_w / src.width)), Image.Resampling.LANCZOS)
    front = captioned(front, pieces[0][1], "center", front_w / src.width)
    backs = []
    back_h = None
    for i, (path, caption) in enumerate(pieces[1:3]):
        src = Image.open(path).convert("RGBA")
        # As in `compose`: the backs share the first one's height, so a narrower still is
        # narrower, not taller.
        bw = back_w if back_h is None else round(src.width * back_h / src.height)
        b = src.resize((bw, round(src.height * bw / src.width)), Image.Resampling.LANCZOS)
        back_h = back_h or b.height
        if i == 0:
            b = captioned(b, caption, "left", bw / src.width, edge="top").rotate(2.5, expand=True, resample=Image.Resampling.BICUBIC)
        else:
            b = captioned(b, caption, "right", bw / src.width).rotate(-2.5, expand=True, resample=Image.Resampling.BICUBIC)
        backs.append(b)
    top = backs[0]
    front_y = round(top.height * 0.45)
    bottom_y = front_y + round(front.height * 0.6)
    height = bottom_y + (backs[1].height if len(backs) > 1 else 0) + 4 * SCALE
    canvas = Image.new("RGBA", (W, max(height, front_y + front.height + 4 * SCALE)), (0, 0, 0, 0))
    canvas.alpha_composite(top, (0, 0))
    if len(backs) > 1:
        canvas.alpha_composite(backs[1], (W - backs[1].width, bottom_y))
    canvas.alpha_composite(front, ((W - front_w) // 2, front_y))
    canvas = canvas.crop(canvas.getbbox())
    canvas = canvas.resize((PORTRAIT_OUT_W, round(canvas.height * PORTRAIT_OUT_W / canvas.width)), Image.Resampling.LANCZOS)
    canvas.save(out, optimize=True)
    print(f"{out}: {canvas.width}x{canvas.height}")


def compose_portrait_fan(out, pieces):
    """The tall picture with more than two backs: the first half of them (rounded up) fanned in
    a row above the front one, their captions on their top edges, the rest in a row under its
    lower half, their captions on their bottom edges. A row of two stands against the edges,
    tilted apart; a row of three has its middle one a little further out of the front's way
    (higher above, lower below) and drawn first, so its caption shows between the others."""
    W = PORTRAIT_W
    front_w = round(W * 0.88)
    src = Image.open(pieces[0][0]).convert("RGBA")
    front = src.resize((front_w, round(src.height * front_w / src.width)), Image.Resampling.LANCZOS)
    front = captioned(front, pieces[0][1], "center", front_w / src.width)
    rest = pieces[1:]
    half = (len(rest) + 1) // 2

    def row(items, above):
        k = len(items)
        cards = []
        for i, (path, caption) in enumerate(items):
            middle = k == 3 and i == 1
            align = "center" if middle else ("left" if i == 0 else "right")
            if k == 1:
                align = "left" if above else "right"
            bw = round(W * (0.78 if k == 1 else 0.56 if middle else 0.6))
            img = Image.open(path).convert("RGBA")
            b = img.resize((bw, round(img.height * bw / img.width)), Image.Resampling.LANCZOS)
            b = captioned(b, caption, align, bw / img.width, edge="top" if above else "bottom")
            tilt = 0.0 if middle else (2.5 if align == "left" else -2.5)
            b = b.rotate(tilt, expand=True, resample=Image.Resampling.BICUBIC)
            x = (W - b.width) // 2 if middle else (0 if align == "left" else W - b.width)
            cards.append((b, x, middle))
        return cards

    top, bottom = row(rest[:half], True), row(rest[half:], False)
    shift = round(W * 0.06)
    top_h = max(b.height for b, _, _ in top) + shift
    front_y = round(top_h * 0.45)
    bottom_y = front_y + round(front.height * 0.6)
    height = max(front_y + front.height, bottom_y + max(b.height for b, _, _ in bottom) + shift) + 4 * SCALE
    canvas = Image.new("RGBA", (W, height), (0, 0, 0, 0))
    for b, x, middle in sorted(top, key=lambda c: not c[2]):
        canvas.alpha_composite(b, (x, 0 if middle else shift))
    for b, x, middle in sorted(bottom, key=lambda c: not c[2]):
        canvas.alpha_composite(b, (x, bottom_y + (shift if middle else 0)))
    canvas.alpha_composite(front, ((W - front_w) // 2, front_y))
    canvas = canvas.crop(canvas.getbbox())
    canvas = canvas.resize((PORTRAIT_OUT_W, round(canvas.height * PORTRAIT_OUT_W / canvas.width)), Image.Resampling.LANCZOS)
    canvas.save(out, optimize=True)
    print(f"{out}: {canvas.width}x{canvas.height}")


def main(argv):
    if argv and argv[0] in ("compose", "compose-portrait"):
        if len(argv) < 4:
            sys.exit(__doc__)
        pieces = []
        for arg in argv[2:]:
            path, _, caption = arg.rpartition(":")
            pieces.append((Path(path), caption))
        return (compose_portrait if argv[0] == "compose-portrait" else compose)(Path(argv[1]), pieces)
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
