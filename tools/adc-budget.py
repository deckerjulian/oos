#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""adc-budget.py: the error budget of the interleaved ADC cores on a unit (issue #335, step 0).

The built-in generator's sine goes to one channel (cable GI/GO -> CHn). For each frequency and V/div the tool takes
one record of 100 k points at the channel's full rate, once with the cores as they come (`:ACQuire:CORes RAW`) and
once matched (`MATC`), and asks the scope for the budget of that record (`:SYSTem:DEBug:CORes?`, computed by
`oos_dsp::interleave::budget`): each core's offset, gain and time error, the offset, gain and timing patterns, the
harmonics, the generator's image at 156.25 MHz - f, the noise, and SINAD with and without the patterns.

It changes the scope's settings (generator, channel, time base, depth, cores, Run/Stop) and leaves the generator off
and the cores matched at the end; run it only on a scope whose owner asked for it. Python's standard library only.

  tools/adc-budget.py --host IP [--channel 1] [--channels 1] [--freq 1e6,10e6,50e6] [--vdiv 0.2,0.5] [--vpp 2]
"""
import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import oosclient  # noqa: E402

# the generator's sample rate: its image of a sine at f lies at GEN_FS - f (docs/bandwidth-test.md, test D)
GEN_FS = 156.25e6
# cores of one channel by the number of channels on (adc-mode.md 6): 1 -> 8, 2 -> 4, 3 or 4 -> 2
CORES = {1: 8, 2: 4, 3: 2, 4: 2}
POINTS = 100_000


def floats(text):
    return [float(x) for x in text.split(",") if x.strip()]


def record(link, single_timeout=5.0):
    """One new record: Single, then wait until it has stopped."""
    link.send(":SING")
    end = time.time() + single_timeout
    while time.time() < end:
        if link.query(":TRIGger:STATus?").strip().upper().startswith("STOP"):
            return True
        time.sleep(0.05)
    return False


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--host", required=True)
    ap.add_argument("--port", type=int, default=5555)
    ap.add_argument("--channel", type=int, default=1, choices=[1, 2, 3, 4], help="the channel the generator is cabled to")
    ap.add_argument("--channels", type=int, default=1, choices=[1, 2, 3, 4], help="channels on (1 = the full rate)")
    ap.add_argument("--freq", type=floats, default=[1e6, 10e6, 50e6], help="sine frequencies, Hz")
    ap.add_argument("--vdiv", type=floats, default=[0.2, 0.5], help="volts per division")
    ap.add_argument("--vpp", type=float, default=2.0, help="generator amplitude, Vpp")
    a = ap.parse_args()
    link = oosclient.Link(a.host, a.port, timeout=20.0)
    m = CORES[a.channels]
    on = [a.channel] + [c for c in range(1, 5) if c != a.channel][: a.channels - 1]
    for c in range(1, 5):
        link.send(f":CHANnel{c}:DISPlay {'ON' if c in on else 'OFF'}")
    rate = 1.25e9 / {8: 1, 4: 2, 2: 4}[m]
    # one plain record per Single: Normal acquisition, no segments, an edge trigger on the generator's channel
    link.send(":ACQuire:TYPE NORMal")
    link.send(":ACQuire:SEGMents OFF")
    link.send(":TRIGger:MODE EDGE")
    link.send(f":ACQuire:MDEPth {POINTS}")
    link.send(f":TIMebase:SCALe {POINTS / rate / 10:.6g}")
    link.send(f":TRIGger:EDGE:SOURce CHANnel{a.channel}")
    link.send(":SOURce1:FUNCtion SIN")
    link.send(f":SOURce1:VOLTage {a.vpp}")
    link.send(":SOURce1:OUTPut ON")
    print("# V/div f/Hz cores sinad matched offset gain timing harmonics image noise  (dB, dBc)")
    try:
        for vdiv in a.vdiv:
            link.send(f":CHANnel{a.channel}:SCALe {vdiv}")
            for f in a.freq:
                link.send(f":SOURce1:FREQuency {f}")
                link.send(":TRIGger:EDGE:LEVel 0")
                time.sleep(0.5)
                for cores in ("RAW", "MATC"):
                    link.send(f":ACQuire:CORes {cores}")
                    if not record(link):
                        print(f"{vdiv} {f:.6g} {cores}: no trigger")
                        continue
                    ans = link.query(f":SYSTem:DEBug:CORes? {a.channel},{m},{GEN_FS - f:.6g}").strip()
                    if ans == "none":
                        print(f"{vdiv} {f:.6g} {cores}: {ans}")
                        continue
                    head, *per_core = ans.split(";")
                    kv = dict(x.split("=", 1) for x in head.split(","))
                    print(f"{vdiv} {f:.6g} {cores} {kv['sinad']} {kv['matched']} {kv['offset']} {kv['gain']} "
                          f"{kv['timing']} {kv['harmonics']} {kv.get('extra', '-')} {kv['noise']}")
                    print(f"#   cores (offset V, gain, time s): {' '.join(per_core)}")
    finally:
        link.send(":SOURce1:OUTPut OFF")
        link.send(":ACQuire:CORes MATC")
        link.send(":RUN")
        link.close()


if __name__ == "__main__":
    main()
