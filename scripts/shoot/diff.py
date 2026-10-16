#!/usr/bin/env python3
"""Prints the fraction of pixels (0 to 1) that differ between two captures of the same size:
arkos-shoot takes two a moment apart and keeps a frame only when they agree, since a window
caught mid-repaint (Claude Code redraws a whole pane at a time) shows half of two frames.

    python3 -I scripts/shoot/diff.py a.png b.png"""
import sys

from PIL import Image, ImageChops

a, b = (Image.open(p).convert("RGB") for p in sys.argv[1:3])
if a.size != b.size:
    print(1.0)
    sys.exit(0)
diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0)
changed = sum(1 for v in diff.get_flattened_data() if v) if hasattr(diff, "get_flattened_data") else sum(1 for v in diff.getdata() if v)
print(f"{changed / (a.width * a.height):.5f}")
