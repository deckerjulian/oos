#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Validate PR6 SIM integration in a loopback OOS desktop app, without hardware.

Replaces R8 with generated records and leaves SIM stopped. Uses the identity
guard and standard-library transport from scpi_validate.py. No GitHub calls.
"""

import argparse
import datetime
import json
import math
from pathlib import Path
import time
import urllib.parse

from scpi_validate import OOS, Results


def ready(oos, seconds=45):
    deadline = time.monotonic() + seconds
    while oos.query(":SIM:BUSY?") == "1":
        if time.monotonic() >= deadline:
            raise RuntimeError("SIM generation did not finish within 45 seconds")
        time.sleep(0.05)


def live(oos):
    oos.request("/cmd?" + urllib.parse.urlencode({"c": "workflow live"}))


def run(oos, result):
    oos.sock.settimeout(15)
    oos.write("*CLS")
    live(oos)
    oos.write(':SIM:PRESET "PCIe 3.0";:SIM:SLOT R8;:SIM:SSC:MODE DOWN;:SIM:SSC:FREQ 33000;:SIM:SSC:DEV 0.005;:SIM:JITT:DCD 0;:SIM:JITT:CORR 0;:SIM:PATT PRBS31')
    started = time.monotonic()
    oos.write(":SIM:PLAY ON")
    result.add("background generation keeps HTTP responsive", oos.state()["mode"] in ("live", "sim"))
    ready(oos)
    elapsed = time.monotonic() - started
    oos.write(":STOP;:WAV:SOUR R8")
    result.equal("full SSC selects R8", oos.state()["source"], "R8")
    result.equal("SSC reference session is SIM", oos.state()["mode"], "sim")
    points = int(oos.query(":WAV:POIN?"))
    expected = math.ceil(8 * 8e9 / 33000)
    result.equal("PCIe3 covers one full 33kHz SSC cycle", points, expected)
    result.equal("full SSC exceeds old short record", points > 65536, True)
    result.equal("stopped SIM and Play agree", oos.query(":SIM:PLAY?"), "0")
    result.add("generated reference has measurable voltage", 0.5 < oos.measure("VPP", "R8") < 1.5)
    result.equal("SSC config query", oos.query(":SIM:SSC:MODE?"), "DOWN")
    result.numeric("SSC frequency in hertz", float(oos.query(":SIM:SSC:FREQ?")), 33000, absolute=0.01)
    result.numeric("SSC deviation as fraction", float(oos.query(":SIM:SSC:DEV?")), 0.005, absolute=1e-9)

    eye_started = time.monotonic()
    oos.write(":VIEW EYE")
    deadline = time.monotonic() + 30
    edges = 0.0
    while time.monotonic() < deadline:
        edges = float(oos.query(":EYE:ITEM? EDGES"))
        if math.isfinite(edges) and 1000 < edges < 9e30:
            break
        time.sleep(0.05)
    eye_elapsed = time.monotonic() - eye_started
    result.add("zoomed trace Eye analyzes full stored record", math.isfinite(edges) and 1000 < edges < 9e30,
               full_record_edges=edges)
    result.numeric("full-record Eye uses 8Gbps unit interval", float(oos.query(":EYE:ITEM? UI")), 1 / 8e9, relative=0.01)
    result.equal("immutable Eye record is folded once", float(oos.query(":EYE:ITEM? RECORDS")), 1.0)
    oos.write(":VIEW SCOPE")

    # Configure all added features together. Generation must use the settings,
    # while the separate DSP regressions check their numerical edge effects.
    oos.write(":SIM:SSC:MODE CENTER;:SIM:SSC:DEV 0.003;:SIM:SSC:FREQ 40000;:SIM:JITT:DCD 4e-12;:SIM:JITT:CORR 2e-12;:SIM:JITT:CORN 2e6;:SIM:PATT PRBS13;:SIM:PLAY ON")
    ready(oos)
    oos.write(":STOP")
    result.equal("centered SSC generates full cycle", int(oos.query(":WAV:POIN?")), math.ceil(8 * 8e9 / 40000))
    result.equal("centered SSC and PRBS13 query", oos.query(":SIM:SSC:MODE?;:SIM:PATT?"), "CENTER;PRBS13")
    for command, expected in [(":SIM:JITT:DCD?", 4e-12), (":SIM:JITT:CORR?", 2e-12), (":SIM:JITT:CORN?", 2e6)]:
        result.numeric(command, float(oos.query(command)), expected, relative=1e-6)
    result.equal("SIM additions have no SCPI error", oos.query(":SYST:ERR?").split(",")[0], "0")

    # A full period that would exceed the explicit budget must fail before
    # replacing the last good waveform, without launching a worker.
    held = oos.state()
    held_points = int(oos.query(":WAV:POIN?"))
    oos.write(":SIM:SSC:FREQ 1000;:SIM:PLAY ON")
    result.equal("oversized full cycle reports parameter error", oos.query(":SYST:ERR?").split(",")[0], "-224")
    result.equal("oversized request does not generate", oos.query(":SIM:BUSY?"), "0")
    result.equal("oversized request preserves waveform", int(oos.query(":WAV:POIN?")), held_points)
    result.equal("oversized request preserves session", oos.state(), held)

    # Return to Live must invalidate an in-flight result, so it cannot re-enter
    # SIM after the user has left. This checks actual worker completion.
    oos.write(":SIM:SSC:FREQ 33000;:SIM:PLAY ON")
    live(oos)
    ready(oos)
    time.sleep(max(1.0, elapsed * 1.3))
    result.equal("cancelled generation cannot return from Live", oos.state()["mode"], "live")
    result.equal("Live clears replay Play", oos.query(":SIM:PLAY?"), "0")

    # SSC off retains the compact record and ordinary Run/Stop behavior.
    oos.write(":SIM:SSC:MODE OFF;:SIM:PLAY ON")
    ready(oos)
    oos.write(":STOP;:WAV:SOUR R8")
    result.equal("SSC off uses compact record", int(oos.query(":WAV:POIN?")), 65536)
    result.equal("final acquisition stopped", oos.state()["running"], False)
    result.equal("final SCPI error queue", oos.query(":SYST:ERR?").split(",")[0], "0")
    return {"full_ssc_points": points, "full_ssc_generation_seconds": elapsed,
            "full_record_eye_seconds": eye_elapsed, "final_state": oos.state()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scpi-port", type=int, default=5555)
    parser.add_argument("--web-port", type=int, default=18080)
    parser.add_argument("--output", type=Path, default=Path("local/sim-validation.json"))
    args = parser.parse_args()
    report = {"created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    result = Results()
    oos = None
    try:
        oos = OOS("127.0.0.1", args.scpi_port, args.web_port)
        report["identity"] = oos.idn
        report["runtime"] = run(oos, result)
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        report["error"] = str(error)
        print(f"ERROR {error}")
    finally:
        if oos is not None:
            oos.close()
    report["checks"] = result.checks
    report["passed"] = sum(check["passed"] for check in result.checks)
    report["failed"] = sum(not check["passed"] for check in result.checks)
    report["ok"] = not report.get("error") and not report["failed"] and bool(result.checks)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(f"{report['passed']} passed, {report['failed']} failed; evidence: {args.output}")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
