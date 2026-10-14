#!/usr/bin/env python3
"""Prints the dominant colours of a PNG, overall and in the bands a VS Code window has, so the
Navy theme's chrome can be sampled from docs/reference/ (spec 7). Standard library only; a
Retina screenshot takes about a minute. Usage: python3 -I scripts/sample-png.py <file.png>"""
import struct
import sys
import zlib
from collections import Counter


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w, h, bpp = 8, [], 0, 0, 0
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype in (2, 6) and interlace == 0, f"unsupported PNG: depth {depth} type {ctype} interlace {interlace}"
            bpp = 3 if ctype == 2 else 4
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    raw = zlib.decompress(b"".join(idat))
    stride = w * bpp
    rows, prev, p = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    return w, h, bpp, rows


def top(rows, bpp, x0, x1, y0, y1, n=4):
    counts = Counter()
    for y in range(y0, y1):
        row = rows[y]
        for x in range(x0, x1):
            counts[row[x * bpp:x * bpp + 3]] += 1
    total = sum(counts.values()) or 1
    return [(f"#{px.hex()}", round(100 * k / total, 1)) for px, k in counts.most_common(n)]


w, h, bpp, rows = read_png(sys.argv[1])
print(f"{w}x{h}")
# Band sizes are doubled for a 2x (Retina) capture.
bands = [
    ("whole", (0, w, 0, h)),
    ("title bar (top 80)", (0, w, 0, min(80, h))),
    ("tab bar (80 under title)", (0, w, 80, min(160, h))),
    ("status bar (bottom 48)", (0, w, max(0, h - 48), h)),
    ("activity bar (left 96)", (0, min(96, w), 80, h - 48)),
    ("sidebar (96 to 600)", (96, min(600, w), 80, h - 48)),
    ("editor area", (600, w, 80, h - 48)),
]
for name, box in bands:
    print(f"{name:24}", top(rows, bpp, *box))
