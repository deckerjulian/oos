#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""frame-check.py: quick look at frames read by `oos-fpgad run --execute` (raw little-endian u16 words, one channel).

    frame-check.py FRAME [FRAME2 ...]

Per frame: words, min/max, mid level, rising/falling crossings of the mid level (with hysteresis), median period and
duty in samples. With several frames: words that differ from the first frame. Identical frames mean the acquisition
was not re-armed (play-last of the old record, see adc-spu-init.md); a live square wave gives shifted edges.
"""
import struct
import sys


def load(p):
    b = open(p, "rb").read()
    return list(struct.unpack("<%dH" % (len(b) // 2), b[: len(b) // 2 * 2]))


def med5(w):
    """Median of 5 neighbours: removes isolated outlier words (warm-reboot bit flips, adc-spu-init.md)."""
    return [sorted(w[max(0, i - 2): i + 3])[len(w[max(0, i - 2): i + 3]) // 2] for i in range(len(w))]


def describe(p, w):
    f = med5(w)
    srt = sorted(f)
    lo, hi = srt[len(srt) // 100], srt[-1 - len(srt) // 100]
    mid, hyst = (lo + hi) / 2, max((hi - lo) / 10, 50)
    outl = sum(1 for a, b in zip(w, f) if abs(a - b) > max((hi - lo) / 4, 500))
    st, rise, fall = f[0] > mid, [], []
    for i, v in enumerate(f):
        if not st and v > mid + hyst:
            st, _ = True, rise.append(i)
        elif st and v < mid - hyst:
            st, _ = False, fall.append(i)
    s = f"{p}: {len(w)} words, 1%..99% {lo}..{hi}, {len(rise)} rising / {len(fall)} falling edges, {outl} outlier words"
    per = sorted(b - a for a, b in zip(rise, rise[1:]))
    if per:
        per = per[len(per) // 2]
        highs = sorted(min(x for x in fall if x > r) - r for r in rise if any(x > r for x in fall))
        s += f", period {per} samples"
        if highs:
            s += f", duty {100 * highs[len(highs) // 2] / per:.0f} %"
    if hi - lo < 200:
        s += "  (flat: no signal or 0 V)"
    return s


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    frames = [(p, load(p)) for p in sys.argv[1:]]
    for p, w in frames:
        print(describe(p, w))
    p0, w0 = frames[0]
    for p, w in frames[1:]:
        n = min(len(w0), len(w))
        d = sum(1 for a, b in zip(w0[:n], w[:n]) if a != b)
        print(f"{p} vs {p0}: {d} of {n} words differ" + ("  (IDENTICAL: not re-armed?)" if d == 0 else ""))


if __name__ == "__main__":
    main()
