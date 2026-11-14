#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Validate a local OOS desktop build against analytic waveforms (no hardware).

    python tools/scpi_validate.py --web-port 18080 --scpi-port 5555

Replaces R1-R3 with synthetic references and leaves acquisition stopped. Refuses
non-loopback addresses and any server whose identity does not start with OOS,.
Uses only Python's standard library. Results default to local/scpi-validation.json.

OOS implements a Rigol-style subset plus extensions, NOT TekScope SCPI. The
TekScope manual documents the measurement concepts used here: SCREEN gating
(page 664), reference sources (806), and 10/90 versus 20/80 thresholds (791).
These tests compare OOS to mathematical truth, not a physical scope or TekScope.
The fixed tolerances below account for sample rounding, one boundary sample,
and threshold interpolation; they must not be widened to accommodate failures.
"""

import argparse
import datetime
import ipaddress
import json
import math
from pathlib import Path
import socket
import sys
import time
import urllib.parse
import urllib.request


TEKSCOPE_MANUAL = "https://download.tek.com/manual/TekScope-Programmer-Manual_077177904.pdf"
SAMPLE_INTERVAL = 1e-9
PERIOD = 1e-6
RAMP_TIME = 100e-9
POINTS = 50000


class OOS:
    def __init__(self, host, scpi_port, web_port):
        if not ipaddress.ip_address(host).is_loopback:
            raise ValueError("This validation is for a loopback OOS desktop build only")
        self.sock = socket.create_connection((host, scpi_port), timeout=5)
        self.stream = self.sock.makefile("rb")
        self.base = f"http://{host}:{web_port}"
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.idn = self.query("*IDN?")
        if not self.idn.startswith("OOS,"):
            self.close()
            raise ValueError(f"Refusing to change non-OOS server: {self.idn!r}")

    def close(self):
        self.stream.close()
        self.sock.close()

    def query(self, command):
        return self.query_bytes(command).decode("ascii").strip()

    def query_bytes(self, command):
        self.sock.sendall((command + "\n").encode("ascii"))
        # An IEEE block is #9, nine digits, then that many bytes. Any other answer is one line.
        # Read a single byte first: a one-character answer such as "0" must not swallow its newline.
        first = self.stream.read(1)
        if not first:
            raise RuntimeError(f"No SCPI response to {command!r}")
        if first != b"#":
            rest = self.stream.readline(1 << 20)
            line = first + rest
            if not line.endswith(b"\n"):
                raise RuntimeError(f"Incomplete SCPI response to {command!r}")
            return line[:-1]
        second = self.stream.read(1)
        if second != b"9":
            rest = self.stream.readline(1 << 20)
            line = first + second + rest
            if not line.endswith(b"\n"):
                raise RuntimeError(f"Incomplete SCPI response to {command!r}")
            return line[:-1]
        digits = self.stream.read(9)
        try:
            n = int(digits)
        except ValueError as error:
            raise RuntimeError(f"Bad block length from {command!r}: {digits!r}") from error
        payload = b""
        while len(payload) < n:
            chunk = self.stream.read(n - len(payload))
            if not chunk:
                raise RuntimeError(f"Incomplete block from {command!r}")
            payload += chunk
        if self.stream.read(1) != b"\n":
            raise RuntimeError(f"Block from {command!r} was not terminated")
        return first + second + digits + payload

    def write(self, command):
        self.sock.sendall((command + "\n").encode("ascii"))
        if self.query("*OPC?") != "1":
            raise RuntimeError(f"Command did not complete: {command}")

    def request(self, path, data=None):
        headers = {"Origin": self.base}
        if data is not None:
            headers["Content-Type"] = "application/octet-stream"
        request = urllib.request.Request(self.base + path, data=data, headers=headers)
        with self.http.open(request, timeout=15) as response:
            return response.read()

    def state(self):
        return json.loads(self.request("/ui-state"))

    def upload(self):
        # Three independent signals, with the same absolute time grid. Construct
        # sample phase from integer indexes to avoid accumulated floating error.
        rows = ["Time (s),Sine (V),Trapezoid (V),Changing amplitude (V)\n"]
        for index in range(POINTS):
            phase = index % 1000
            sine = math.sin(2 * math.pi * phase / 1000)
            if phase < 100:
                trapezoid = -0.5 + 2 * phase / 100
            elif phase < 500:
                trapezoid = 1.5
            elif phase < 600:
                trapezoid = 1.5 - 2 * (phase - 500) / 100
            else:
                trapezoid = -0.5
            amplitude = 1 if index < 25000 else 3
            rows.append(f"{index * SAMPLE_INTERVAL:.12g},{0.5 + 2 * sine:.12g},"
                        f"{trapezoid:.12g},{amplitude * sine:.12g}\n")
        path = "/waveform?" + urllib.parse.urlencode({"name": "analytic-validation.csv", "slot": 1})
        return self.request(path, "".join(rows).encode("ascii")).decode("utf-8").strip()

    def window(self, centre):
        self.write(":STOP")
        self.write(":TIMebase:SCALe 1e-6")
        self.write(f":TIMebase:OFFSet {centre:.12g}")
        # SCPI writes are consumed before the next display/analysis tick. Wait
        # for that tick rather than reading the previous measurement window.
        time.sleep(0.2)

    def measure(self, item, source):
        return float(self.query(f":MEASure:ITEM? {item},{source}"))

    def wait_state(self, predicate, seconds=2):
        deadline = time.monotonic() + seconds
        while True:
            state = self.state()
            if predicate(state) or time.monotonic() >= deadline:
                return state
            time.sleep(0.05)


class Results:
    def __init__(self):
        self.checks = []

    def numeric(self, name, actual, expected, absolute=0.0, relative=0.0, **extra):
        tolerance = max(absolute, abs(expected) * relative)
        self.add(name, math.isfinite(actual) and abs(actual - expected) <= tolerance,
                 actual=actual, expected=expected, tolerance=tolerance, **extra)

    def equal(self, name, actual, expected):
        self.add(name, actual == expected, actual=actual, expected=expected)

    def add(self, name, passed, **details):
        self.checks.append({"name": name, "passed": bool(passed), **details})
        print(f"{'PASS' if passed else 'FAIL'} {name}")


def validate(oos, result):
    oos.write("*CLS")
    result.equal("clean initial error queue", oos.query(":SYSTem:ERRor?").split(",")[0], "0")
    upload = oos.upload()
    oos.window(10e-6)
    state = oos.state()
    result.equal("HTTP upload selects R1", state["source"], "R1")
    result.equal("reference acquisition is stopped", state["running"], False)
    result.equal("reference measurements use screen gate", state["gate"], "screen")
    result.equal("selected reference slot", oos.query(":REFerence:SLOT?"), "R1")
    result.numeric("timebase query", float(oos.query(":TIMebase:SCALe?")), 1e-6, relative=1e-6)
    result.numeric("delay query", float(oos.query(":TIMebase:OFFSet?")), 10e-6, relative=1e-6)

    # y(t)=0.5+2 sin(2 pi 1MHz t): extrema +/-2 around 0.5,
    # mean=0.5 and RMS=sqrt(0.5^2+2^2/2)=1.5 over integer periods.
    for item, expected, absolute, relative in [
        ("VPP", 4.0, 2e-5, 0), ("VMAX", 2.5, 2e-5, 0),
        ("VMIN", -1.5, 2e-5, 0), ("VAVG", 0.5, 5e-4, 0),
        ("VRMS", 1.5, 5e-4, 0), ("FREQuency", 1e6, 0, 2e-4),
        ("PERiod", PERIOD, 0, 2e-4),
    ]:
        result.numeric(f"sine {item}", oos.measure(item, "R1"), expected,
                       absolute=absolute, relative=relative, source="R1")

    # A symmetric trapezoid has 100ns linear ramps and 400ns plateaus.
    # Its average is 0.5V, amplitude 2V. Mean square is
    # offset^2 + half_amplitude^2 * (1 - 4*ramp/(3*period)).
    for item, expected, absolute, relative in [
        ("VPP", 2.0, 2e-5, 0), ("VAMP", 2.0, 2e-3, 0),
        ("VAVG", 0.5, 5e-4, 0),
        ("VRMS", math.sqrt(0.25 + 1 - 4 * RAMP_TIME / (3 * PERIOD)), 5e-4, 0),
        ("FREQuency", 1e6, 0, 2e-4), ("PERiod", PERIOD, 0, 2e-4),
    ]:
        result.numeric(f"trapezoid {item}", oos.measure(item, "R2"), expected,
                       absolute=absolute, relative=relative, source="R2")

    for thresholds, fraction in [("10/90", 0.8), ("20/80", 0.6)]:
        oos.write(":MEASure:REFS " + thresholds)
        result.equal(f"threshold query {thresholds}", oos.query(":MEASure:REFS?"), thresholds)
        for item in ["RTIMe", "FTIMe"]:
            result.numeric(f"trapezoid {item} {thresholds}", oos.measure(item, "R2"),
                           fraction * RAMP_TIME, absolute=1.1e-9, source="R2")
    oos.write(":MEASure:REFS 10/90")

    result.numeric("source isolation R1", oos.measure("VPP", "R1"), 4.0, absolute=2e-5)
    result.numeric("source isolation REF2 alias", oos.measure("VPP", "REF2"), 2.0, absolute=2e-5)
    oos.write(":REFerence:SLOT R2")
    oos.write(":REFerence:SHOW OFF")
    result.numeric("hidden reference remains measurable", oos.measure("VPP", "R2"), 2.0, absolute=2e-5)
    oos.write(":REFerence:SHOW ON")
    oos.write(":CHANnel1:DISPlay OFF")
    result.equal("disabled CH1 has no valid measurement", oos.measure("VPP", "CHAN1"), 9.9e37)
    result.equal("no unexpected errors after numeric tests", oos.query(":SYSTem:ERRor?").split(",")[0], "0")

    # All reference sources share the visible time window. R3 is +/-1V before
    # 25us and +/-3V after 25us, with integer-period boundaries in both windows.
    oos.write(":REFerence:SLOT R1")
    oos.window(10e-6)
    before = oos.state()
    result.numeric("stopped left window VPP", oos.measure("VPP", "R3"), 2.0, absolute=2e-5)
    oos.window(35e-6)
    after = oos.state()
    result.numeric("stopped right window VPP", oos.measure("VPP", "R3"), 6.0, absolute=2e-5)
    result.equal("panning does not acquire", after["captures"], before["captures"])
    result.equal("panning keeps STOP", oos.query(":TRIGger:STATus?"), "STOP")
    result.equal("panning keeps stopped workflow", after["running"], False)

    for command, name, expects_response in [
        (":MEASure:ITEM? VPP,R11", "invalid source", True),
        (":MEASure:ITEM? NOT_A_MEASUREMENT,R1", "invalid item", True),
        (":MEASure:REFS 30/70", "invalid thresholds", False),
        (":REFerence:SLOT R11", "invalid slot", False),
    ]:
        oos.write("*CLS")
        if expects_response:
            result.equal(name + " returns invalid sentinel", float(oos.query(command)), 9.9e37)
        else:
            oos.write(command)
        result.equal(name + " reports parameter error", oos.query(":SYSTem:ERRor?").split(",")[0], "-224")
        result.equal(name + " error is consumed", oos.query(":SYSTem:ERRor?").split(",")[0], "0")
    result.equal("bad threshold preserves 10/90", oos.query(":MEASure:REFS?"), "10/90")
    result.equal("bad slot preserves R1", oos.query(":REFerence:SLOT?"), "R1")

    # Acquisition commands must share the UI's reference replay/trigger state.
    # An impossible level waits without capturing. A real crossing completes
    # Single exactly once; continuous Run advances and Stop holds the counter.
    oos.window(10e-6)
    oos.write(":TRIGger:EDGE:SOURce CHANnel1")
    oos.write(":TRIGger:EDGE:SLOPe POS")
    oos.write(":TRIGger:EDGE:LEVel 100")
    baseline = oos.state()["captures"]
    oos.write(":SINGle")
    armed = oos.wait_state(lambda state: state["status"] == "WAIT")
    result.equal("Single arms reference replay", armed["running"], True)
    result.equal("impossible trigger waits", armed["status"], "WAIT")
    result.equal("impossible trigger acquires nothing", armed["captures"], baseline)
    oos.write(":TRIGger:EDGE:LEVel 0.5")
    single = oos.wait_state(lambda state: not state["running"] and state["captures"] > baseline)
    result.equal("Single captures exactly once", single["captures"], baseline + 1)
    result.equal("Single stops after trigger", single["running"], False)
    result.equal("Single records triggered status", single["status"], "TRIGGERED")
    result.equal("SCPI Single completion is STOP", oos.query(":TRIGger:STATus?"), "STOP")
    oos.write(":TRIGger:SWEep NORMal")
    oos.write(":RUN")
    running = oos.wait_state(lambda state: state["captures"] >= single["captures"] + 2)
    result.equal("Run reports running", running["running"], True)
    result.add("Run advances reference acquisitions", running["captures"] >= single["captures"] + 2,
               before=single["captures"], after=running["captures"])
    result.equal("Run and replay Play agree", oos.query(":SIM:PLAY?"), "1")
    observed = []
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        observed.append(oos.measure("VPP", "R3"))
        if max(observed) - min(observed) > 3:
            break
        time.sleep(0.1)
    result.add("running measurements follow changing waveform windows",
               max(observed) - min(observed) > 3, observed_vpp=observed,
               expected="reference R3 spans 2Vpp and 6Vpp regions as captures advance")
    oos.write(":STOP")
    stopped = oos.state()
    time.sleep(0.25)
    held = oos.state()
    result.equal("Stop holds capture count", held["captures"], stopped["captures"])
    result.equal("Stop and replay Play agree", oos.query(":SIM:PLAY?"), "0")
    result.equal("final error queue", oos.query(":SYSTem:ERRor?").split(",")[0], "0")

    # The commands a Rigol DHO800/DHO900 controller sends. A query must always
    # answer; an unknown header is an error in the queue and in *ESR, never silence.
    oos.write("*CLS")
    unknown = oos.query(":REFerence:NOPE?")
    result.equal("unknown query answers", unknown.split(",")[0], "-113")
    result.equal("unknown query sets the command-error bit", oos.query("*ESR?"), "32")
    result.equal("unknown query is queued", oos.query(":SYSTem:ERRor?").split(",")[0], "-113")

    oos.write(":CHANnel1:DISPlay ON")
    oos.write(":CHANnel1:SCALe 0.5")
    oos.write(":CHANnel1:OFFSet -0.25")
    oos.write(":CHANnel1:COUPling AC")
    oos.write(":CHANnel1:BWLimit 20M")
    result.equal("channel display", oos.query(":CHANnel1:DISPlay?"), "1")
    result.equal("channel coupling", oos.query(":CHANnel1:COUPling?"), "AC")
    result.equal("channel bandwidth", oos.query(":CHANnel1:BWLimit?"), "20M")
    result.numeric("channel scale", float(oos.query(":CHANnel1:SCALe?")), 0.5, relative=1e-6)
    oos.write(":CHANnel1:PROBe 10")
    result.numeric("channel probe", float(oos.query(":CHANnel1:PROBe?")), 10.0, relative=1e-6)
    oos.write(":TIMebase:MAIN:SCALe 1e-6")
    oos.write(":TRIGger:SWEep NORMal")
    oos.write(":TRIGger:EDGE:SOURce CHANnel1")
    oos.write(":TRIGger:EDGE:SLOPe NEG")
    result.equal("trigger sweep", oos.query(":TRIGger:SWEep?"), "NORM")
    result.equal("trigger slope", oos.query(":TRIGger:EDGE:SLOPe?"), "NEG")
    result.equal("no live record is an invalid measurement", oos.measure("VPP", "CHANnel1"), 9.9e37)

    png = oos.query_bytes(":DISPlay:DATA? PNG")
    result.equal("png screenshot is an IEEE block", png[:2], b"#9")
    result.equal("png screenshot is a PNG", png[11:15], b"\x89PNG")

    oos.write(":REFerence:CURRent 1")
    oos.write(":REFerence:SOURce 1,CHANnel2")
    oos.write(":REFerence:VSCale 1,0.2")
    oos.write(":REFerence:VOFFset 1,-0.05")
    oos.write(":REFerence:LABel:CONTent 1,lane")
    result.equal("reference current", oos.query(":REFerence:CURRent?"), "1")
    result.equal("reference source", oos.query(":REFerence:SOURce? 1"), "CHAN2")
    result.equal("reference label", oos.query(":REFerence:LABel:CONTent? 1"), "lane")
    result.numeric("reference scale", float(oos.query(":REFerence:VSCale? 1")), 0.2, relative=1e-6)
    oos.write(":REFerence:SHOW 1,OFF")
    result.equal("reference hidden", oos.query(":REFerence:SHOW? 1"), "0")
    oos.write(":REFerence:SHOW 1,ON")

    oos.write(":WAVeform:SOURce R1")
    oos.write(":WAVeform:MODE RAW")
    oos.write(":WAVeform:FORMat WORD")
    oos.write(":WAVeform:STARt 10")
    oos.write(":WAVeform:STOP 12")
    result.equal("raw page length", oos.query(":WAVeform:POINts?"), "3")
    page = oos.query_bytes(":WAVeform:DATA?")
    result.equal("raw WORD page length field", page[2:11], b"000000006")
    result.equal("raw WORD page is three high-byte-first points", len(page), 17)

    oos.write(":CURSor:MODE MANual")
    oos.write(":CURSor:MANual:TYPE TIME")
    oos.write(":CURSor:MANual:CAX 0")
    oos.write(":CURSor:MANual:CBX 1e-6")
    result.equal("cursor mode", oos.query(":CURSor:MODE?"), "MAN")
    result.equal("cursor type", oos.query(":CURSor:MANual:TYPE?"), "TIME")
    result.numeric("cursor A time", float(oos.query(":CURSor:MANual:CAX?")), 0.0, absolute=1e-12)
    result.numeric("cursor delta", float(oos.query(":CURSor:MANual:XDELta?")), 1e-6, relative=1e-4)
    oos.write(":CURSor:MODE OFF")
    result.equal("cursors off have no delta", float(oos.query(":CURSor:MANual:XDELta?")), 9.9e37)

    oos.write(":MEASure:CLEar")
    oos.write(":MEASure:SOURce R1")
    oos.write(":MEASure:ITEM VPP")
    result.equal("measurement source", oos.query(":MEASure:SOURce?"), "R1")
    strip = oos.query(":MEASure:STRip?")
    result.add("measure item is on the readout", strip.startswith("VPP,R1,"), strip=strip)
    oos.write(":MEASure:CLEar")
    result.equal("measure clear empties the readout", oos.query(":MEASure:STRip?"), "")

    oos.write(":CHANnel1:LABel:CONTent Vbus")
    oos.write(":CHANnel1:UNITs AMPere")
    oos.write(":CHANnel1:VERNier ON")
    result.equal("channel label", oos.query(":CHANnel1:LABel:CONTent?"), "Vbus")
    result.equal("channel units", oos.query(":CHANnel1:UNITs?"), "AMP")
    result.equal("channel vernier", oos.query(":CHANnel1:VERNier?"), "1")
    oos.write(":TIMebase:HREFerence:MODE TRIGger")
    oos.write(":TIMebase:DELay:ENABle ON")
    oos.write(":TIMebase:DELay:SCALe 1e-7")
    oos.write(":TIMebase:XY:ENABle ON")
    result.equal("zoom and xy", oos.query(":TIMebase:HREFerence:MODE?;:TIMebase:DELay:ENABle?;:TIMebase:XY:ENABle?"), "TRIG;1;1")
    result.numeric("zoom scale", float(oos.query(":TIMebase:DELay:SCALe?")), 1e-7, relative=1e-3)
    oos.write(":TIMebase:XY:ENABle OFF")
    oos.write(":TIMebase:DELay:ENABle OFF")

    oos.write(":MEASure:STATistic:DISPlay ON")
    oos.write(":MEASure:STATistic:ITEM VPP,R1")
    result.equal("statistics on", oos.query(":MEASure:STATistic:DISPlay?"), "1")
    result.add("statistic current is a number",
               math.isfinite(float(oos.query(":MEASure:STATistic:ITEM? CURRent,VPP,R1"))))
    oos.write(":MEASure:STATistic:RESet")
    result.equal("statistic reset", float(oos.query(":MEASure:STATistic:ITEM? CNT,VPP,R1")), 0.0)

    oos.write(":ACQuire:TYPE AVERages")
    oos.write(":ACQuire:AVERages 100")
    oos.write(":ACQuire:ULTRa:MAXFrame 100")
    result.equal("acquire type and averages", oos.query(":ACQuire:TYPE?;:ACQuire:AVERages?;:ACQuire:ULTRa:MAXFrame?"), "AVER;64;100")
    oos.write(":ACQuire:TYPE HRESolution")
    result.equal("high resolution acquire", oos.query(":ACQuire:TYPE?"), "HRES")
    oos.write(":CHANnel1:TCALibrate 2e-8")
    result.numeric("channel deskew", float(oos.query(":CHANnel1:TCALibrate?")), 2e-8, relative=1e-6)
    oos.write(":REFerence:COLor 1,RED")
    result.equal("reference color", oos.query(":REFerence:COLor? 1"), "RED")
    oos.write(":MASK:ENABle ON")
    oos.write(":MASK:SOURce CHANnel1")
    oos.write(":MASK:X 0.5")
    oos.write(":MASK:Y 0.5")
    result.equal("mask setup", oos.query(":MASK:ENABle?;:MASK:SOURce?;:MASK:OPERate?"), "1;CHAN1;STOP")
    result.numeric("mask horizontal tolerance", float(oos.query(":MASK:X?")), 0.5, relative=1e-6)
    result.numeric("mask vertical tolerance", float(oos.query(":MASK:Y?")), 0.5, relative=1e-6)
    oos.write(":MASK:ENABle OFF")

    oos.write(":MEASure:SETup:MIN 20")
    oos.write(":MEASure:SETup:MAX 80")
    result.equal("measure thresholds", oos.query(":MEASure:SETup:MIN?;:MEASure:SETup:MAX?"), "20;80")

    oos.write(":DVM:ENABle ON")
    oos.write(":DVM:SOURce CHANnel2")
    oos.write(":DVM:MODE ACRMS")
    oos.write(":COUNter:ENABle ON")
    oos.write(":COUNter:MODE PERiod")
    oos.write(":COUNter:NDIGits 6")
    result.equal("dvm and counter setup",
                 oos.query(":DVM:ENABle?;:DVM:SOURce?;:DVM:MODE?;:COUNter:ENABle?;:COUNter:MODE?;:COUNter:NDIGits?"),
                 "1;CHAN2;ACRMS;1;PER;6")
    return {"upload": upload, "before_pan": before, "after_pan": after,
            "armed": armed, "single": single, "running": running, "final_state": held}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--scpi-port", type=int, default=5555)
    parser.add_argument("--web-port", type=int, default=18080)
    parser.add_argument("--output", type=Path, default=Path("local/scpi-validation.json"))
    args = parser.parse_args()
    report = {
        "created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "target": {"host": args.host, "scpi_port": args.scpi_port, "web_port": args.web_port},
        "scope": "OOS custom Rigol-style subset; Rn and MEAS:REFS are OOS extensions; no TekScope compatibility claim",
        "documentation": {"manual": TEKSCOPE_MANUAL, "gating_page": 664,
                          "thresholds_page": 791, "reference_source_page": 806,
                          "syntax": "Probewright command_router, Rigol DHO800/DHO900, :MEASure:ITEM?"},
        "fixtures": {"sample_interval_s": SAMPLE_INTERVAL, "points": POINTS,
                     "period_s": PERIOD, "linear_ramp_s": RAMP_TIME,
                     "R1": "0.5 + 2*sin(2*pi*t/period)",
                     "R2": "-0.5..1.5 symmetric trapezoid, 100ns ramps, 400ns plateaus",
                     "R3": "sin(2*pi*t/period), amplitude 1 before 25us and 3 thereafter"},
    }
    result = Results()
    oos = None
    try:
        oos = OOS(args.host, args.scpi_port, args.web_port)
        report["identity"] = oos.idn
        report["runtime"] = validate(oos, result)
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        report["error"] = str(error)
        print(f"ERROR {error}", file=sys.stderr)
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
    sys.exit(main())
