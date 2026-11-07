#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""Reference fixture for hal/src/tpu.rs from tools/tpumodel.py (read by `tpu::tests::matches_tpumodel`).

Runs the model for a fixed list of trigger set-ups and writes the ordered register writes and the end shadows to
hal/testdata/tpu_expected.txt. One block per case:

  == <step> ; <step> ...      each step `type key=value ...` with every parameter of the model function resolved
                              (levels in nV, times in fs, plus the level parameters scale/offset/g/noise; video: vclk)
  0xREG 0xVALUE               one line per write, in order
  sh 0x2014=0x... ...         end shadows, sorted by register

The steps of one case run on the same shadows, starting from `Tpu.APP` (so a case can show what a type inherits from
the previous one). `holdoff t_fs=N` is `Tpu.holdoff` on its own.

  tpu_fixture.py        # rewrite hal/testdata/tpu_expected.txt
  tpu_fixture.py -      # print to stdout
"""

import inspect
import os
import sys

import tpumodel as m

US = 1_000_000_000          # 1 us in fs
NS = 1_000_000              # 1 ns in fs

CASES = [
    # edge (configuration, levels, holdoff)
    "edge ch=1 slope=rising",
    "edge ch=1 slope=falling",
    "edge ch=2 slope=any level=1e9 scale=2e8 offset=-3e8 g=1.07 noise=0.7",
    "edge ch=3 slope=falling level=-2.5e8 scale=1e8 holdoff_fs=30000000000000000",
    "edge ch=4 slope=rising level=4e9 scale=1e9 g=0.93 holdoff_fs=1000000000000",
    # pulse
    f"pulse ch=1 pol=pos when=gt lower={100 * NS}",
    f"pulse ch=2 pol=neg when=lt upper={50 * NS} level=3e8",
    f"pulse ch=3 pol=pos when=in lower={20 * NS} upper={80 * NS} scale=2e8 offset=1e8",
    f"pulse ch=4 pol=neg when=out lower={1 * US} upper={2 * US} noise=0.7",
    f"pulse ch=1 pol=any when=gt lower={100 * NS}",
    f"pulse ch=1 pol=pos when=gt lower=10000000000000000 upper=20000000000000000",
    f"pulse ch=12 pol=neg when=lt upper={30 * NS}",
    f"edge ch=1 slope=falling ; pulse ch=2 pol=any when=lt upper={40 * NS}",
    f"pulse ch=1 pol=pos when=gt lower={100 * NS} ; holdoff t_fs=100000000000",
    # slope
    f"slope ch=1 slope=rising when=lt upper={20 * NS} hi=1e9 lo=2e8",
    f"slope ch=2 slope=falling when=gt lower={10 * NS} hi=5e8 lo=-5e8 scale=2e8",
    f"slope ch=1 slope=rising when=in lower={5 * NS} upper={15 * NS}",
    f"slope ch=3 slope=falling when=out lower={5 * NS} upper={15 * NS} g=1.05 noise=0.7",
    # runt
    "runt ch=1 pol=pos when=none hi=2e9 lo=8e8",
    f"runt ch=1 pol=neg when=gt lower={30 * NS} hi=2e9 lo=8e8",
    f"runt ch=2 pol=any when=lt upper={30 * NS}",
    f"runt ch=1 pol=pos when=in lower={10 * NS} upper={40 * NS} from_settype=0",
    f"runt ch=4 pol=neg when=out lower={10 * NS} upper={40 * NS} scale=1e8 offset=-2e8",
    "runt ch=3 pol=any when=none lower=123456789 upper=987654321",
    # window
    "window ch=1 slope=rising event=enter hi=1e9 lo=-1e9",
    "window ch=1 slope=rising event=exit",
    f"window ch=1 slope=rising event=time time={1 * US}",
    "window ch=2 slope=falling event=enter",
    "window ch=2 slope=falling event=exit",
    f"window ch=2 slope=falling event=time time={1 * US}",
    "window ch=3 slope=any event=enter scale=2e8",
    "window ch=3 slope=any event=exit",
    f"window ch=1 slope=any event=time time={1 * US}",
    "window ch=4 slope=rising event=enter from_settype=0",
    # timeout
    f"timeout ch=1 slope=rising time={1 * US}",
    f"timeout ch=2 slope=falling time={5 * US} level=-1e8",
    f"timeout ch=3 slope=any time={250 * NS}",
    # delay
    f"delay srca=1 slopea=rising srcb=2 slopeb=falling when=gt lower={50 * NS}",
    f"delay srca=2 slopea=falling srcb=1 slopeb=rising when=lt upper={20 * NS} levela=5e8 levelb=-5e8",
    f"delay srca=3 slopea=rising srcb=4 slopeb=rising when=in lower={10 * NS} upper={100 * NS}",
    f"delay srca=1 slopea=falling srcb=2 slopeb=falling when=out lower={10 * NS} upper={100 * NS}",
    f"delay srca=1 slopea=rising srcb=9 slopeb=falling when=gt lower={50 * NS}",
    # pattern
    "pattern pats=H,L,R,X logic=and",
    "pattern pats=H,H,X,X logic=or level=1e9",
    "pattern pats=L,X,F,H logic=nand",
    "pattern pats=X,X,X,X logic=nor",
    "pattern pats=F,X,X,X logic=and",
    "pattern pats=X,X,X,X,X,X,X,X,X,R logic=or",
    "pattern pats=X,X,X,X,R,X logic=and",
    "edge ch=2 slope=falling ; pattern pats=H,L,X,X logic=or",
    # duration
    f"duration pats=H,H,X,X logic=and when=gt lower={1 * US}",
    f"duration pats=L,H,X,X logic=or when=lt upper={2 * US}",
    f"duration pats=X,R,X,X logic=and when=in lower={1 * US} upper={3 * US}",
    f"duration pats=F,X,X,X logic=nor when=out lower={1 * US} upper={3 * US}",
    f"duration pats=H,X,L,H logic=nand when=gt lower={500 * NS}",
    f"edge ch=3 slope=rising ; duration pats=H,L,X,X logic=and when=gt lower={1 * US}",
    # setup/hold
    f"setuphold clk=1 data=2 slope=rising dtype=H event=setup setup={5 * NS}",
    f"setuphold clk=1 data=2 slope=rising dtype=H event=hold hold={2 * NS}",
    f"setuphold clk=1 data=2 slope=rising dtype=H event=both setup={5 * NS} hold={2 * NS}",
    f"setuphold clk=2 data=1 slope=falling dtype=L event=setup setup={8 * NS} levelc=1e9 leveld=-1e9",
    f"setuphold clk=2 data=1 slope=falling dtype=L event=hold hold={3 * NS}",
    f"setuphold clk=3 data=4 slope=falling dtype=L event=both setup={10 * NS} hold={4 * NS}",
    f"setuphold clk=4 data=3 slope=rising dtype=L event=both setup={10 * NS} hold={4 * NS}",
    f"setuphold clk=1 data=9 slope=rising dtype=H event=both setup={5 * NS} hold={2 * NS}",
    # Nth edge
    f"nth ch=1 slope=rising count=3 idle={1 * US}",
    f"nth ch=1 slope=falling count=5 idle={2 * US}",
    "nth ch=2 slope=rising count=100 idle=0 level=7e8 scale=1e9",
    "nth ch=4 slope=falling count=65536 idle=30000000000000000",
    f"nth ch=1 slope=rising count=3 idle={1 * US} ; edge ch=1 slope=rising",
    # video
    "video ch=1 std=pal polarity=0 sync=line line=100",
    "video ch=1 std=pal polarity=0 sync=line line=100 vclk=stock",
    "video ch=1 std=ntsc polarity=0 sync=any",
    "video ch=2 std=ntsc polarity=1 sync=odd",
    "video ch=2 std=ntsc polarity=1 sync=even vclk=stock",
    "video ch=3 std=pal polarity=1 sync=odd",
    "video ch=4 std=pal polarity=0 sync=even vclk=stock",
    "video ch=1 std=ntsc polarity=0 sync=line line=20 vclk=stock",
    "video ch=1 std=pal polarity=1 sync=any level=2e8",
    "edge ch=1 slope=falling ; video ch=1 std=pal polarity=0 sync=line line=100",
    "edge ch=1 slope=any ; window ch=1 slope=rising event=enter ; nth ch=2 slope=falling count=4 idle=0"
    " ; video ch=2 std=ntsc polarity=0 sync=any ; pulse ch=1 pol=any when=gt lower=100000000",
]

FUNCS = {n: getattr(m, n) for n in ("edge", "pulse", "slope", "runt", "window", "timeout", "delay", "pattern",
                                     "duration", "setuphold", "nth", "video")}
LV = ("scale", "offset", "g", "noise")
PAT_NAME = {v: k for k, v in m.PAT.items()}
LOGIC_NAME = {v: k for k, v in m.LOGIC.items()}


def fmt(v):
    if isinstance(v, bool):
        return str(int(v))
    if isinstance(v, float):
        return repr(v)
    return str(v)


def run_step(t, spec):
    """Runs one step on `t` and returns its resolved text form."""
    typ, *kv = spec.split()
    given = dict(x.split("=", 1) for x in kv)
    if typ == "holdoff":
        t.holdoff(int(given["t_fs"]))
        return "holdoff t_fs=%d" % int(given["t_fs"])
    f = FUNCS[typ]
    lv = m._lv({k: float(given.pop(k)) for k in LV if k in given})
    clk = given.pop("vclk", "fast") if typ == "video" else "fast"
    args, text = {}, [typ]
    for name, p in list(inspect.signature(f).parameters.items())[1:]:
        if name == "lv":
            continue
        s = given.pop(name, None)
        if name == "pats":
            v = [m.PAT[x] for x in s.split(",")]
            text.append("pats=" + ",".join(PAT_NAME[x] for x in v))
        elif name == "logic":
            v = m.LOGIC[s] if s is not None else p.default
            text.append("logic=" + LOGIC_NAME[v])
        else:
            if s is None:
                if p.default is inspect.Parameter.empty:
                    raise SystemExit("%s: %s missing" % (spec, name))
                v = p.default
            elif isinstance(p.default, bool):
                v = s == "1"
            elif isinstance(p.default, float):
                v = float(s)
            elif isinstance(p.default, int) or p.default is None or p.default is inspect.Parameter.empty:
                v = int(s) if s.lstrip("-").isdigit() else s
            else:
                v = s
            text.append("%s=%s" % (name, fmt(v)))
        args[name] = v
    if given:
        raise SystemExit("%s: unknown %s" % (spec, ", ".join(given)))
    text += ["%s=%s" % (k, fmt(lv[k])) for k in LV]
    if typ == "video":
        t.video_clk = m.VIDEO_CLK_FS[clk]
        text.append("vclk=" + clk)
    f(t, lv=lv, **args)
    return " ".join(text)


def main():
    out = []
    for case in CASES:
        t = m.Tpu()
        steps = [run_step(t, s.strip()) for s in case.split(";")]
        out.append("== " + " ; ".join(steps))
        out += ["0x%04x 0x%08x" % (r, v) for r, v, _ in t.w]
        out.append("sh " + " ".join("0x%04x=0x%08x" % (r, v) for r, v in sorted(t.sh.items())))
    text = "\n".join(out) + "\n"
    if sys.argv[1:] == ["-"]:
        sys.stdout.write(text)
        return
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "hal", "testdata", "tpu_expected.txt")
    with open(path, "w") as fh:
        fh.write(text)
    print("%d cases, %d writes -> %s" % (len(CASES), sum(1 for x in out if x.startswith("0x")), os.path.normpath(path)))


if __name__ == "__main__":
    main()
