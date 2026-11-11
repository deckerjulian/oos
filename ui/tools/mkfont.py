#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""mkfont.py: render a UI font atlas from Roboto (tools/fonts, Apache License 2.0, see tools/fonts/LICENSE-Roboto.txt).
Glyphs: ASCII 32..126 plus a few symbols, each an 8-bit alpha bitmap with metrics relative to the baseline.

Usage: mkfont.py OUT [normal|large|larger]. One atlas per text size (Display > Text size): normal is
assets/font.bin, large (every face 2 px bigger) assets/font-large.bin, larger (4 px bigger) assets/font-larger.bin.
The output depends only on the TTFs and Pillow's rasteriser: running it again gives the same bytes.

Format: "OOSF", u16 font count, then per font: <HHHHI size ascent descent nglyphs alphalen>, nglyphs records
<IhhhhhhI cp w h x0 y0 adv 0 off>, alpha bytes."""
import os, struct, sys
from PIL import ImageFont, Image, ImageDraw
HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fonts")
# (field name in font.rs, face, pixel size of the normal set); order = order in Fonts::load
BASE = [("s", "Regular", 12), ("sb", "Medium", 12), ("m", "Regular", 14), ("mb", "Medium", 14),
        ("l", "Regular", 20), ("lb", "Medium", 18), ("xl", "Regular", 26)]
GROW = {"normal": 0, "large": 2, "larger": 4}
SET = sys.argv[2] if len(sys.argv) > 2 else "normal"
FONTS = [(name, face, size + GROW[SET]) for name, face, size in BASE]
EXTRA = ["µ", "±", "°", "Ω", "×", "Δ", "²", "·", "σ", "…"]  # µ ± ° Ω × Δ ² · σ …
CHARS = [chr(c) for c in range(32, 127)] + EXTRA
blob = bytearray(b"OOSF") + struct.pack("<H", len(FONTS))
for name, face, size in FONTS:
    f = ImageFont.truetype(os.path.join(HERE, f"Roboto-{face}.ttf"), size)
    asc, desc = f.getmetrics()
    data = bytearray(); glyphs = []
    for ch in CHARS:
        adv = round(f.getlength(ch))
        x0, y0, x1, y1 = f.getbbox(ch, anchor="ls")          # relative to (left, baseline)
        w, h = max(0, x1 - x0), max(0, y1 - y0)
        off = len(data)
        if w and h:
            im = Image.new("L", (w, h), 0)
            ImageDraw.Draw(im).text((-x0, -y0), ch, font=f, fill=255, anchor="ls")
            data += im.tobytes()
        glyphs.append((ord(ch), w, h, x0, y0, adv, off))
    blob += struct.pack("<HHHHI", size, asc, desc, len(glyphs), len(data))
    for g in glyphs:
        blob += struct.pack("<IhhhhhhI", g[0], g[1], g[2], g[3], g[4], g[5], 0, g[6])
    blob += data
open(sys.argv[1], "wb").write(blob)
print("font atlas", SET, "written", sys.argv[1], len(blob), "bytes")
