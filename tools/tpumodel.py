#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""TPU (trigger unit, BAR1 0x2000 block) register model.

Prints the ordered register writes for one trigger type, plus the end state of
the software shadows. See docs/protocols/trigger.md sections 2 (edge) and 10 (other types). No
vendor code is used; every function below re-states the observed behaviour of one register primitive or one trigger
type.

Units: times in femtoseconds internally (CLI takes seconds), voltages in nV at the BNC (CLI takes volts).

Examples:
  tpumodel.py edge --ch 1 --slope rising                      # 0x201c = 0x00aa4100, 0x205c = 0 (= hal/src/dho/tpu.rs)
  tpumodel.py pulse --ch 1 --pol pos --when gt --lower 100e-9
  tpumodel.py slope --ch 2 --slope rising --when lt --upper 20e-9 --hi 1.0 --lo 0.2
  tpumodel.py runt --ch 1 --pol pos --hi 2.0 --lo 0.8
  tpumodel.py window --ch 1 --slope rising --event enter --hi 1 --lo -1
  tpumodel.py timeout --ch 1 --slope rising --time 1e-6
  tpumodel.py delay --srca 1 --slopea rising --srcb 2 --slopeb falling --when gt --lower 50e-9
  tpumodel.py pattern --pat H,L,R,X --logic and
  tpumodel.py duration --pat H,H,X,X --when gt --lower 1e-6
  tpumodel.py setuphold --clk 1 --data 2 --slope rising --dtype H --event setup --setup 5e-9
  tpumodel.py nth --ch 1 --slope rising --count 3 --idle 1e-6
  tpumodel.py video --ch 1 --std pal --sync line --line 100
  tpumodel.py selftest
"""

import argparse
import struct
import sys

# ----------------------------------------------------------------------------------------------------------------
# constants

TICK_FS = 1_600_000          # event block time unit: 1.6 ns
TICK64_FS = 6_400_000        # holdoff / R timeout / delay time unit: 6.4 ns
VIDEO_CLK_FS = {"fast": 4_000_000, "stock": 6_400_000}  # FPGA clock cycle: 4 ns or 6.4 ns

WHEN = {"gt": 0, "lt": 1, "in": 2, "out": 3, "timeout": 4, "none": 5}   # TPU codes 0x201c[26:24]
API_WHEN = {"none": 0, "gt": 1, "lt": 2, "in": 3, "out": 4}              # time condition


def to_tpu_when(api):
    """TPU code of a time condition: none -> 5, more than -> 0, less than -> 1, inside -> 2, outside -> 3, else 6."""
    if api == 0:
        return 5
    return api - 1 if api < 5 else 6


def trunc_div(a, b):
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


# ----------------------------------------------------------------------------------------------------------------
# source / comparator codes

def cmp_a(src):
    if 1 <= src <= 4:
        return 2 * src - 2
    if 9 <= src <= 24:
        return src + 0x17
    return {25: 48, 26: 49}.get(src, 51)


def cmp_b(src):
    if 1 <= src <= 4:
        return 2 * src - 1
    if 9 <= src <= 24:
        return src + 0x17
    return {25: 48, 26: 49}.get(src, 51)


def digital_or_ext(src):
    """The bypass test used by pulse/slope/timeout/delay/setup-hold/Nth: 9 <= src < 28 (D0..D15, AC line, EXT)."""
    return 1 if 9 <= src < 0x1c else 0


LEVEL_REGS = {1: (0x2004, 0x2008), 2: (0x200c, 0x2010), 3: (0x20b8, 0x20bc), 4: (0x20c0, 0x20c4)}


# ----------------------------------------------------------------------------------------------------------------
# TPU model: shadows + primitives

class Tpu:
    # stock values in RUN/STOP; 0x204c/0x2014/0x2018 not in the snapshot -> 0
    APP = {0x201c: 0x00aa4100, 0x2034: 0x6060, 0x204c: 0, 0x205c: 0, 0x2088: 0x40000, 0x208c: 0,
           0x2014: 0, 0x2018: 0}

    def __init__(self, shadows=None, video_clk="fast"):
        self.sh = dict(self.APP if shadows is None else shadows)
        self.w = []          # (reg, value, comment)
        self.video_clk = VIDEO_CLK_FS[video_clk]

    def wr(self, reg, val, note=""):
        self.w.append((reg, val & 0xffffffff, note))

    # --- event A/B/R configuration (0x201c / 0x2034 / 0x204c blocks) ---
    EV = {"A": 0x201c, "B": 0x2034, "R": 0x204c}

    def set_source(self, ev, a, ea, b, eb):
        r = self.EV[ev]
        lo = (a & 0x3f) | (ea & 3) << 6 | (b & 0x3f) << 8 | (eb & 3) << 14
        self.sh[r] = (self.sh[r] & 0xffff0000) | lo
        self.wr(r, self.sh[r], "source %s (%d,%d,%d,%d)" % (ev, a, ea, b, eb))

    def set_pattern(self, ev, p0, p1, p2, p3):
        r = self.EV[ev]
        f = (p0 & 7) << 16 | (p1 & 1) << 19 | (p2 & 7) << 20 | (p3 & 1) << 23
        self.sh[r] = (self.sh[r] & 0xff00ffff) | f
        self.wr(r, self.sh[r], "pattern %s (%d,%d,%d,%d)" % (ev, p0, p1, p2, p3))

    def set_trigger(self, ev, when, t1_fs, t2_fs):
        r = self.EV[ev]
        t1, t2 = trunc_div(t1_fs, TICK_FS), trunc_div(t2_fs, TICK_FS)
        self.sh[r] = (self.sh[r] & ~0x07000000 & 0xffffffff) | (when & 7) << 24
        self.wr(r, self.sh[r], "trigger %s (when=%d, t1=%d, t2=%d ticks)" % (ev, when, t1, t2))
        self.wr(r + 4, t1, "  t1[31:0]")
        self.wr(r + 8, t2, "  t2[31:0]")
        self.wr(r + 12, ((t1 >> 32) & 0xffff) | ((t2 >> 32) & 0xffff) << 16, "  t1[47:32] | t2[47:32]<<16")

    # --- 0x205c ---
    def _w205c(self, note):
        self.wr(0x205c, self.sh[0x205c], note)

    def set_mode(self, m):
        self.sh[0x205c] = (self.sh[0x205c] & ~0x1f) | (m & 0x1f)
        self._w205c("mode (%d)" % m)

    def set_bypass(self, b, note=""):
        self.sh[0x205c] = (self.sh[0x205c] & ~0x1000) | (b & 1) << 12
        self._w205c("bypass (%d)%s" % (b, note))

    def set_abr_fsm(self, a, b, c, d, e):
        f = (a & 1) << 6 | (b & 1) << 7 | (c & 1) << 8 | (d & 1) << 9 | (e & 1) << 10
        self.sh[0x205c] = (self.sh[0x205c] & ~0x7c0) | f
        self._w205c("abr (%d,%d,%d,%d,%d)" % (a, b, c, d, e))

    # --- misc ---
    def set_iir(self, c, src, k=0x800):
        v = (c & 3) | ((src - 1) & 3) << 2 | (k & 0x3fff) << 4
        self.sh[0x208c] = (self.sh[0x208c] & 0xfffc0000) | v
        self.wr(0x208c, self.sh[0x208c], "iir (%d, src %d, 0x%x)" % (c, src, k))

    def set_logic_a(self, idx, val, op):
        """Logic entry: idx 0..15 -> 0x2014 (written), 16..20 -> 0x2018[9:0] (shadow only), then 0x2018 op."""
        if idx <= 15:
            s = 2 * idx
            self.sh[0x2014] = (self.sh[0x2014] & ~(3 << s) & 0xffffffff) | (val << s)
            self.wr(0x2014, self.sh[0x2014], "logic (%d,%d,%d)" % (idx, val, op))
        elif idx <= 20:
            s = 2 * (idx - 16)
            lo = self.sh[0x2018] & 0xffff
            new = (lo & ~(3 << s)) | (val << s)
            self.sh[0x2018] = (self.sh[0x2018] & 0xfffffc00) | (new & 0x3ff)
        self.sh[0x2018] = (self.sh[0x2018] & ~0xc00) | (op & 3) << 10
        self.wr(0x2018, self.sh[0x2018], "  0x2018 (inputs 16..20, op %d)%s" % (op, "" if idx <= 15 else " idx %d" % idx))

    def holdoff(self, t_fs):
        t = trunc_div(t_fs, TICK64_FS)
        self.sh[0x2088] = (self.sh[0x2088] & ~0x70000) | 0x40000
        self.wr(0x2088, self.sh[0x2088], "holdoff type (4)")
        self.wr(0x2084, t, "holdoff time %d x 6.4 ns" % t)
        self.sh[0x2088] = (self.sh[0x2088] & 0xffff0000) | ((t >> 32) & 0xffff)
        self.wr(0x2088, self.sh[0x2088], "  holdoff[47:32]")

    def r_timeout(self, t_fs):
        t = trunc_div(t_fs, TICK64_FS)
        self.wr(0x2068, t, "R timeout %d x 6.4 ns" % t)
        self.wr(0x206c, t >> 32, "  high 32 bits")

    def video_cfg1(self, mode, line):
        self.wr(0x21a4, (mode & 3) | (line & 0x7ff) << 2, "video1 (%d, line %d)" % (mode, line))

    def video_cfg2(self, t1_fs, t2_fs):
        a, b = trunc_div(t1_fs, self.video_clk), trunc_div(t2_fs, self.video_clk)
        self.wr(0x21a8, (a & 0xffffffff & ~0xffff0000) | (b & 0xffff) << 16,
                "video2 (%g us, %g us) / %g ns" % (t1_fs / 1e9, t2_fs / 1e9, self.video_clk / 1e6))

    def video_cfg3(self, a, lines, c, d):
        self.wr(0x21ac, (a & 0x7ff) | (lines & 0x7ff) << 11 | (c & 1) << 22 | (d & 1) << 23,
                "video3 (%d, %d, %d, %d)" % (a, lines, c, d))

    # --- levels ---
    @staticmethod
    def code(v, scale, g):
        x = v * 7500.0 / scale / g + 32768.0
        c = 0 if x <= 0 else int(x)
        return min(max(c, 1), 0xfffe) if c else 1

    def level(self, which, ch, v, scale, offset=0.0, g=1.0, noise=0.3, direction=None, add_offset=True):
        """which 'A'/'B'; direction 1 = hysteresis band below the level, 0 = above (default A:1, B:0)."""
        if ch not in LEVEL_REGS:
            return
        if direction is None:
            direction = 1 if which == "A" else 0
        vv = v + (offset if add_offset else 0.0)
        h = int(f32(f32(noise) * f32(scale)))
        if direction:
            up, lo = self.code(vv, scale, g), self.code(vv - h, scale, g)
        else:
            up, lo = self.code(vv + h, scale, g), self.code(vv, scale, g)
        reg = LEVEL_REGS[ch][0 if which == "A" else 1]
        self.wr(reg, up | lo << 16, "level %s CH%d %+.4f V dir %d" % (which, ch, v / 1e9, direction))

    # --- output ---
    def dump(self, out=sys.stdout):
        for i, (r, v, n) in enumerate(self.w, 1):
            out.write("%3d  0x%04x = 0x%08x   %s\n" % (i, r, v, n))
        out.write("shadows: " + ", ".join("0x%04x=0x%08x" % (r, v) for r, v in sorted(self.sh.items())) + "\n")


# ----------------------------------------------------------------------------------------------------------------
# per-type sequences. `lv` = level parameters: dict(scale=nV/div, offset=nV, g=digital gain, noise=div)

def _lv(lv):
    d = dict(scale=5e8, offset=0.0, g=1.0, noise=0.3)
    d.update(lv or {})
    return d


def refresh_levels(t, chans, lv):
    """The stock system rewrites level A and B of all four channels; here only the given channels.
    chans: list of (ch, levelA_nV, levelB_nV, dirA, dirB)."""
    p = _lv(lv)
    for ch, va, vb, da, db in chans:
        t.level("A", ch, va, p["scale"], p["offset"], p["g"], p["noise"], da)
        t.level("B", ch, vb, p["scale"], p["offset"], p["g"], p["noise"], db)


def edge(t, ch, slope="rising", level=0.0, lv=None, holdoff_fs=8_000_000):
    """Edge: configuration (DC coupling), levels, holdoff, = hal/src/dho/tpu.rs `edge` (17 writes)."""
    a, b = cmp_a(ch), cmp_b(ch)
    t.set_trigger("A", 0, 0, 0)
    t.set_trigger("B", 0, 0, 0)
    t.set_bypass(1 if ch > 8 else 0)          # src > 8 || coupling != DC; DC assumed here
    if slope == "alternating":
        return                                  # nothing after the bypass write
    if slope == "rising":
        t.set_source("A", a, 0, b, 1)
        t.set_pattern("A", 2, 1, 2, 1)
    else:                                       # falling, any
        t.set_source("A", b, 1, a, 0)
        if slope == "any":
            t.set_source("B", a, 0, b, 1)
        t.set_pattern("A", 2, 0, 2, 0)
        if slope == "any":
            t.set_pattern("B", 2, 1, 2, 1)
    t.set_mode(1 if slope == "any" else 0)
    p = _lv(lv)
    t.level("A", ch, level, p["scale"], p["offset"], p["g"], p["noise"])
    t.level("B", ch, level, p["scale"], p["offset"], p["g"], p["noise"])
    if holdoff_fs is not None:
        t.holdoff(holdoff_fs)


def pulse(t, ch, pol="pos", when="gt", lower=0, upper=0, level=0.0, lv=None):
    """Pulse. pol: pos (0), neg (1), any (2)."""
    a, b = cmp_a(ch), cmp_b(ch)
    t.set_bypass(digital_or_ext(ch))
    refresh_levels(t, [(ch, level, level, 1, 0)], lv)
    t.set_iir(0, ch)
    t.set_trigger("A", WHEN[when], lower, upper)
    if pol == "pos":
        t.set_source("A", b, 1, a, 0)
        t.set_pattern("A", 2, 0, 1, 0)
    elif pol == "neg":
        t.set_source("A", a, 0, b, 1)
        t.set_pattern("A", 2, 1, 1, 0)
    # pol "any": no source/pattern write at all (0x201c[23:0] keep the previous type's value)
    t.set_mode(0)


def slope(t, ch, slope="rising", when="gt", lower=0, upper=0, hi=1e9, lo=0.0, lv=None):
    """Slope, then the time condition of event A, which is written when the condition or width changes
    (the sequence itself never writes it). Level A = high level, level B = low level."""
    a, b = cmp_a(ch), cmp_b(ch)
    s = 0 if slope == "rising" else 1
    t.set_bypass(digital_or_ext(ch))
    refresh_levels(t, [(ch, hi, lo, 1 if s != 1 else 0, 1 if s == 0 else 0)], lv)
    t.set_iir(0, ch)
    if s == 0:
        t.set_source("A", a, 0, b, 0)
        t.set_pattern("A", 2, 1, 1, 0)
    else:
        t.set_source("A", b, 1, a, 1)
        t.set_pattern("A", 2, 0, 1, 0)
    t.set_mode(0)
    t.set_trigger("A", WHEN[when], lower, upper)   # condition or width change


def runt(t, ch, pol="pos", when="none", lower=0, upper=0, hi=1e9, lo=0.0, lv=None, from_settype=True):
    """Runt (the type change writes bypass 0 first; the sequence itself does not touch bit12)."""
    a, b = cmp_a(ch), cmp_b(ch)
    if from_settype:
        t.set_bypass(0, " (type change)")
    refresh_levels(t, [(ch, hi, lo, 1, 0)], lv)
    t.set_iir(0, ch)
    w = WHEN[when]
    if w == 5:
        w, lower, upper = 0, 0, 0
    if pol == "pos":
        t.set_source("A", a, 0, b, 0)
        t.set_pattern("A", 3, 0, 6, 0)
        t.set_trigger("A", w, lower, upper)
        t.set_mode(0)
    elif pol == "neg":
        t.set_source("A", b, 1, a, 1)
        t.set_pattern("A", 3, 1, 6, 0)
        t.set_trigger("A", w, lower, upper)
        t.set_mode(0)
    else:
        t.set_source("A", a, 0, b, 0)
        t.set_pattern("A", 3, 0, 1, 0)
        t.set_trigger("A", w, lower, upper)
        t.set_source("B", b, 1, a, 1)
        t.set_pattern("B", 3, 1, 1, 0)
        t.set_trigger("B", w, lower, upper)
        t.set_mode(1)


def window(t, ch, slope="rising", event="enter", time=0, hi=1e9, lo=-1e9, lv=None, from_settype=True):
    """Window (type 8 'Over')."""
    a, b = cmp_a(ch), cmp_b(ch)
    ev = {"enter": 0, "exit": 1, "time": 2}[event]
    if from_settype:
        t.set_bypass(0, " (type change)")
    refresh_levels(t, [(ch, hi, lo, 1 if ev != 1 else 0, 1 if ev == 1 else 0)], lv)
    t.set_iir(0, ch)
    t.set_source("A", a, 0, b, 0)
    t.set_source("B", b, 0, a, 0)
    P1, P0 = (2, 1, 2, 1), (2, 0, 2, 0)
    tw = (4, time, time) if ev == 2 else (0, 0, 0)
    if slope == "rising":
        t.set_trigger("A", *tw)
        t.set_pattern("A", *(P1 if ev == 0 else P0))
        t.set_mode(0)
    elif slope == "falling":
        t.set_source("A", b, 0, a, 0)
        t.set_trigger("A", *tw)
        t.set_pattern("A", *(P0 if ev == 0 else P1))
        t.set_mode(0)
    else:
        t.set_trigger("A", *tw)
        t.set_trigger("B", *tw)
        t.set_pattern("A", *(P1 if ev == 0 else P0))
        t.set_pattern("B", *(P0 if ev == 0 else P1))
        t.set_mode(1)


def timeout(t, ch, slope="rising", time=0, level=0.0, lv=None):
    """Timeout. slope: rising (0), falling (1), any (3)."""
    a, b = cmp_a(ch), cmp_b(ch)
    t.set_bypass(digital_or_ext(ch))
    refresh_levels(t, [(ch, level, level, 1, 0)], lv)
    t.set_iir(0, ch)
    t.set_source("A", a, 0, b, 0)
    t.set_source("B", b, 0, a, 0)
    t.set_trigger("A", 4, time, 0)
    t.set_trigger("B", 4, time, 0)
    if slope == "rising":
        t.set_pattern("A", 2, 0, 2, 0)
        t.set_mode(0)
    elif slope == "falling":
        t.set_source("A", b, 0, a, 0)
        t.set_pattern("A", 2, 1, 2, 1)
        t.set_mode(0)
    else:
        t.set_pattern("A", 2, 0, 2, 0)
        t.set_pattern("B", 2, 1, 2, 1)
        t.set_mode(1)


def delay(t, srca, slopea, srcb, slopeb, when="gt", lower=0, upper=0, levela=0.0, levelb=0.0, lv=None):
    """Delay: time from the edge on source A to the edge on source B."""
    sa, sb = (0 if slopea == "rising" else 1), (0 if slopeb == "rising" else 1)
    refresh_levels(t, [(srca, levela, levela, 1, 0), (srcb, levelb, levelb, 1, 0)], lv)
    t.set_bypass(1 if (digital_or_ext(srca) or digital_or_ext(srcb)) else 0)
    t.set_iir(0, srca)
    t.set_iir(0, srcb)
    t.set_trigger("A", WHEN[when], lower, upper)
    ca = cmp_a(srca) if sa == 0 else cmp_b(srca)
    cb = cmp_a(srcb) if sb == 0 else cmp_b(srcb)
    t.set_source("A", cb, sb, ca, sa)
    t.set_pattern("A", 1, 0, 1, 0)
    t.set_mode(0)


PAT = {"H": 0, "L": 1, "X": 2, "R": 3, "F": 4}     # pattern entries
LOGIC = {"and": 0, "or": 1, "nand": 2, "nor": 3}


def _logic_sel(p):
    return {0: 2, 1: 3}.get(p, 0)          # H -> 2, L -> 3, X/rise/fall -> 0


def _logic_fun(l):
    return 1 if l == 1 else (2 if l == 3 else 0)   # OR -> 1, NOR -> 2, AND/NAND -> 0


def _pattern_common(t, pats, logic, level, lv):
    """pats: list of pattern entries for CH1..CH4 (index 0..3); D0..D15 (4..19) and EXT (20) = X."""
    arr = list(pats) + [PAT["X"]] * (21 - len(pats))
    refresh_levels(t, [(i + 1, level, level, 1, 0) for i in range(4) if arr[i] != PAT["X"]], lv)
    for ch in (1, 2, 3, 4):
        t.set_iir(0, ch)
    for i in range(21):
        t.set_logic_a(i, _logic_sel(arr[i]), _logic_fun(logic))
    edges = [i for i, p in enumerate(arr) if p in (3, 4)]
    return arr, (edges[0] if edges else -1)


def pattern(t, pats, logic=0, level=0.0, lv=None):
    """Pattern: type change (bypass 1), then the sequence."""
    t.set_bypass(1, " (type change)")
    arr, e = _pattern_common(t, pats, logic, level, lv)
    if e >= 0:
        c = cmp_a(e + 1)
        t.set_source("A", c, 1 if arr[e] != 3 else 0, c, 2)
        t.set_pattern("A", 2, 1 if arr[e] == 3 else 0, 0, 0)
    else:
        t.set_pattern("A", 0, 0, 0, 0)
    t.set_trigger("A", 0, 0, 0)
    t.set_mode(0)


def duration(t, pats, logic=0, when="gt", lower=0, upper=0, level=0.0, lv=None):
    """Duration: type change (bypass 0), then the sequence."""
    t.set_bypass(0, " (type change)")
    arr, e = _pattern_common(t, pats, logic, level, lv)
    if e >= 0:
        c = cmp_a(e)          # index, not index+1, as stock: CH1 edge gives code 51, CH2 edge gives CH1's code
        t.set_source("A", c, 1 if arr[e] != 3 else 0, c, 2)
        t.set_pattern("A", 0, 0, 2, 1 if arr[e] == 3 else 0)
    else:
        t.set_pattern("A", 0, 0, 0, 0)
    t.set_trigger("A", WHEN[when], lower, upper)
    t.set_mode(0)


def setuphold(t, clk, data, slope="rising", dtype="H", event="setup", setup=0, hold=0, levelc=0.0, leveld=0.0,
              lv=None):
    """Setup/hold. dtype H (0) / L (1); event setup (0), hold (1), setup&hold (2)."""
    s = 0 if slope == "rising" else 1
    d = 0 if dtype == "H" else 1
    ev = {"setup": 0, "hold": 1, "both": 2}[event]
    t.set_bypass(1 if (digital_or_ext(clk) or digital_or_ext(data)) else 0)
    refresh_levels(t, [(clk, levelc, levelc, 1, 0), (data, leveld, leveld, 1, 0)], lv)
    t.set_iir(0, clk)
    t.set_iir(0, data)
    cc = cmp_a(clk) if s == 0 else cmp_b(clk)
    ds = cmp_a(data) if d == 0 else cmp_b(data)        # data edge into the valid level
    dh = cmp_a(data) if d == 1 else cmp_b(data)        # data edge out of the valid level
    if ev in (0, 2):
        t.set_source("A", cc, s, ds, d)
        t.set_pattern("A", 2, 1 if s == 0 else 0, 3, d)
        t.set_trigger("A", 1, 0, setup)
    if ev == 1:
        t.set_source("A", dh, 1 - d, cc, s)
        t.set_pattern("A", 2, d, 6, 0)
        t.set_trigger("A", 1, 0, hold)
    if ev == 2:
        t.set_source("B", dh, 1 - d, cc, s)
        t.set_pattern("B", 2, d, 6, 0)
        t.set_trigger("B", 1, 0, hold)
    t.set_mode(1 if ev == 2 else 0)


def nth(t, ch, slope="rising", count=2, idle=0, level=0.0, lv=None):
    """Nth edge (ABR sequencer, mux 3)."""
    a, b = cmp_a(ch), cmp_b(ch)
    s = 0 if slope == "rising" else 1
    t.set_bypass(digital_or_ext(ch))
    refresh_levels(t, [(ch, level, level, 1, 0)], lv)
    t.set_iir(0, ch)
    t.set_source("A", a, s, b, s)
    if s:
        t.set_source("A", b, s, a, s)
        t.set_pattern("A", 2, 0, 2, 0)
    else:
        t.set_pattern("A", 2, 1, 2, 1)
    t.set_trigger("A", 0, 0, 0)
    t.set_trigger("B", 0, 0, 0)
    t.set_abr_fsm(1, 1, 1, 0, 1)
    t.wr(0x2070, count, "abr count (%d)" % count)
    t.r_timeout(idle)
    t.set_mode(3)


VIDEO_STD = {  # standard -> (Cfg2 t1 fs, Cfg2 t2 fs, Cfg3 args, progressive flag)
    "ntsc": (4_700_000_000, 58_790_000_000, (5, 525, 0, 1), 0),
    "pal": (4_700_000_000, 59_300_000_000, (2, 625, 0, 1), 0),
}
VIDEO_ODD_EVEN = {("ntsc", "odd"): 4, ("ntsc", "even"): 267, ("pal", "odd"): 1, ("pal", "even"): 313}


def video(t, ch, std="pal", polarity=0, sync="any", line=1, level=0.0, lv=None):
    """Video (NTSC/PAL only). sync: any (0), line (1), odd (2), even (3); for odd/even the line of the field comes first."""
    a = cmp_a(ch)
    if sync in ("odd", "even"):
        line = VIDEO_ODD_EVEN[(std, sync)]
    syn = {"any": 0, "line": 1, "odd": 2, "even": 3}[sync]
    refresh_levels(t, [(ch, level, level, 1, 0)], lv)
    t.set_iir(0, ch)
    t.set_source("R", a, 0, a, 0)
    t.set_pattern("R", 0, 0, 2, 0)
    t.set_pattern("A", *((2, 1, 2, 1) if polarity else (2, 0, 2, 0)))
    t.set_mode(14)
    t1, t2, c3, prog = VIDEO_STD[std]
    t.video_cfg2(t1, t2)
    t.video_cfg3(*c3)
    t.video_cfg1((1 if syn else 0) | prog << 1, line)


# ----------------------------------------------------------------------------------------------------------------
# self test

def selftest():
    t = Tpu()
    edge(t, 1, "rising", 0.0)
    assert len(t.w) == 17, len(t.w)
    assert t.sh[0x201c] == 0x00aa4100 and t.sh[0x205c] == 0 and t.sh[0x2088] == 0x40000
    assert t.w[12][:2] == (0x2004, 0x77368000) and t.w[13][:2] == (0x2008, 0x800088ca)
    assert t.w[15][:2] == (0x2084, 1)
    t = Tpu()
    edge(t, 1, "falling", 0.0)
    assert t.sh[0x201c] == 0x00220041
    t = Tpu()
    edge(t, 1, "rising", 1e9)
    assert t.w[12][1] == 0xb1ceba98 and t.w[13][1] == 0xba98c362
    # pulse CH1 positive > 100 ns: 0x201c = 0x00120041 | when 0, t1 = 62 ticks
    t = Tpu()
    pulse(t, 1, "pos", "gt", 100_000_000, 0)
    assert t.sh[0x201c] == 0x00120041, hex(t.sh[0x201c])
    assert (0x2020, 62) in [w[:2] for w in t.w]
    # Nth edge: 0x205c = 0x5c3
    t = Tpu()
    nth(t, 1, "rising", 3, 1_000_000_000)
    assert t.sh[0x205c] == 0x5c3, hex(t.sh[0x205c])
    print("selftest ok")


# ----------------------------------------------------------------------------------------------------------------
# CLI

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("type", choices=["edge", "pulse", "slope", "runt", "window", "timeout", "delay", "pattern",
                                     "duration", "setuphold", "nth", "video", "selftest"])
    ap.add_argument("--ch", type=int, default=1)
    ap.add_argument("--slope", default="rising", help="rising|falling|any")
    ap.add_argument("--pol", default="pos", help="pos|neg|any (pulse, runt); 0|1 (video)")
    ap.add_argument("--when", default="gt", help="gt|lt|in|out|none")
    ap.add_argument("--lower", type=float, default=0.0, help="s (t1)")
    ap.add_argument("--upper", type=float, default=0.0, help="s (t2)")
    ap.add_argument("--time", type=float, default=0.0, help="s (timeout, window time)")
    ap.add_argument("--event", default="enter", help="window: enter|exit|time; setuphold: setup|hold|both")
    ap.add_argument("--level", type=float, default=0.0, help="V")
    ap.add_argument("--hi", type=float, default=1.0, help="V (level A)")
    ap.add_argument("--lo", type=float, default=0.0, help="V (level B)")
    ap.add_argument("--scale", type=float, default=0.5, help="V/div")
    ap.add_argument("--offset", type=float, default=0.0, help="V")
    ap.add_argument("--gain", type=float, default=1.0, help="digital gain")
    ap.add_argument("--noise", type=float, default=0.3, help="hysteresis in div (0.3 / 0.7)")
    ap.add_argument("--srca", type=int, default=1)
    ap.add_argument("--srcb", type=int, default=2)
    ap.add_argument("--slopea", default="rising")
    ap.add_argument("--slopeb", default="rising")
    ap.add_argument("--clk", type=int, default=1)
    ap.add_argument("--data", type=int, default=2)
    ap.add_argument("--dtype", default="H")
    ap.add_argument("--setup", type=float, default=0.0)
    ap.add_argument("--hold", type=float, default=0.0)
    ap.add_argument("--pat", default="H,X,X,X", help="CH1..CH4 of H,L,X,R,F")
    ap.add_argument("--logic", default="and")
    ap.add_argument("--count", type=int, default=2)
    ap.add_argument("--idle", type=float, default=0.0)
    ap.add_argument("--std", default="pal")
    ap.add_argument("--sync", default="any")
    ap.add_argument("--line", type=int, default=1)
    ap.add_argument("--holdoff", type=float, default=None, help="s; appended for non-edge types if given")
    ap.add_argument("--video-clk", default="fast", choices=["fast", "stock"])
    a = ap.parse_args(argv)
    if a.type == "selftest":
        selftest()
        return
    fs = lambda s: int(round(s * 1e15))
    nv = lambda v: v * 1e9
    lv = dict(scale=nv(a.scale), offset=nv(a.offset), g=a.gain, noise=a.noise)
    t = Tpu(video_clk=a.video_clk)
    T = a.type
    if T == "edge":
        edge(t, a.ch, a.slope, nv(a.level), lv, fs(a.holdoff) if a.holdoff is not None else 8_000_000)
    elif T == "pulse":
        pulse(t, a.ch, a.pol, a.when, fs(a.lower), fs(a.upper), nv(a.level), lv)
    elif T == "slope":
        slope(t, a.ch, a.slope, a.when, fs(a.lower), fs(a.upper), nv(a.hi), nv(a.lo), lv)
    elif T == "runt":
        runt(t, a.ch, a.pol, a.when if a.when != "gt" or a.lower else "none", fs(a.lower), fs(a.upper),
             nv(a.hi), nv(a.lo), lv)
    elif T == "window":
        window(t, a.ch, a.slope, a.event, fs(a.time), nv(a.hi), nv(a.lo), lv)
    elif T == "timeout":
        timeout(t, a.ch, a.slope, fs(a.time), nv(a.level), lv)
    elif T == "delay":
        delay(t, a.srca, a.slopea, a.srcb, a.slopeb, a.when, fs(a.lower), fs(a.upper), nv(a.level), nv(a.level), lv)
    elif T in ("pattern", "duration"):
        pats = [PAT[x.strip().upper()] for x in a.pat.split(",")]
        if T == "pattern":
            pattern(t, pats, LOGIC[a.logic], nv(a.level), lv)
        else:
            duration(t, pats, LOGIC[a.logic], a.when, fs(a.lower), fs(a.upper), nv(a.level), lv)
    elif T == "setuphold":
        ev = a.event if a.event in ("setup", "hold", "both") else "setup"
        setuphold(t, a.clk, a.data, a.slope, a.dtype.upper(), ev, fs(a.setup), fs(a.hold), nv(a.level), nv(a.level),
                  lv)
    elif T == "nth":
        nth(t, a.ch, a.slope, a.count, fs(a.idle), nv(a.level), lv)
    elif T == "video":
        video(t, a.ch, a.std, 1 if a.pol in ("1", "neg") else 0, a.sync, a.line, nv(a.level), lv)
    if T != "edge" and a.holdoff is not None:
        t.holdoff(fs(a.holdoff))
    t.dump()


if __name__ == "__main__":
    main()
