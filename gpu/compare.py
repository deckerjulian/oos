#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# compare.py CPU_DIR GPU_DIR: the pictures of two runs of the same scripts, pixel by pixel. The GPU blends in floating
# point and the CPU in integers, so a few steps of difference are rounding; more is named with where it is.
import os
import sys

# per colour channel, out of 255: a layer drawn from commands blends in floating point twice (into the layer, the
# layer over the picture), a pixel of a rounded corner over colour-graded traces came out 4 off
TOLERANCE = 4


def load(p):
    data = open(p, 'rb').read()
    head = data.split(b'\n', 3)
    w, h = map(int, head[1].split())
    return w, h, head[3]


def main():
    a_dir, b_dir = sys.argv[1], sys.argv[2]
    names = sorted(n for n in os.listdir(a_dir) if n.endswith('.ppm'))
    bad = 0
    worst_all = 0
    for n in names:
        if not os.path.exists(os.path.join(b_dir, n)):
            print('%-36s missing in %s' % (n, b_dir))
            bad += 1
            continue
        wa, ha, pa = load(os.path.join(a_dir, n))
        wb, hb, pb = load(os.path.join(b_dir, n))
        if (wa, ha) != (wb, hb):
            print('%-36s size %dx%d / %dx%d' % (n, wa, ha, wb, hb))
            bad += 1
            continue
        worst, over, box = 0, 0, None
        for i in range(0, len(pa), 3):
            d = max(abs(pa[i] - pb[i]), abs(pa[i + 1] - pb[i + 1]), abs(pa[i + 2] - pb[i + 2]))
            if d > worst:
                worst = d
            if d > TOLERANCE:
                over += 1
                x, y = (i // 3) % wa, (i // 3) // wa
                box = (x, y, x, y) if box is None else (min(box[0], x), min(box[1], y), max(box[2], x), max(box[3], y))
        worst_all = max(worst_all, worst)
        if over:
            bad += 1
            print('%-36s %6d px off by more than %d (worst %d) in x %d..%d y %d..%d' % (n, over, TOLERANCE, worst, box[0], box[2], box[1], box[3]))
    print('%d pictures, %d differ beyond rounding; largest difference %d' % (len(names), bad, worst_all))
    sys.exit(1 if bad else 0)


main()
