#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""Acceptance test of a running OOS over the network, with nobody at the scope.

  tools/device-test.py --host <ip>                 everything the wiring allows
  tools/device-test.py --host <ip> --tier a0       no cable needed
  tools/device-test.py --host <ip> --only trig     tests whose name contains "trig"
  tools/device-test.py --host <ip> --soak-min 480  with a long run at the end
  tools/device-test.py --tier la --pico /dev/cu.usbmodem101    (needs pyserial)
  tools/device-test.py --tier rf --rf /dev/cu.wchusbserial110
  tools/device-test.py --sim                       against `oos-ui --serve` on this computer (no scope)

Tiers, in this order:
  a0  no signal: identity, record lengths, time bases, zero of every scale, network, limits
  a1  generator output on CH1 (found by itself): accuracy, coupling, invert, delay, trigger types, generator, decoder
  a2  generator on a second channel as well: skew, two-channel triggers, hidden trigger source
  la  logic probe plugged in, last because a hanging read needs someone at the power switch: --pico <port> drives
      D0-D2 from GP2-GP4 of a Pico with the openSciLab firmware
  rf  an HT008 signal source on --rf-ch (CH2) over --rf <port>: 23.5 MHz and up

A test that cannot run for lack of wiring is SKIP. A result that needs a decision is INFO. The results go to
device-test/<time>/ as report.md, results.json and screenshots.

It only talks to the published interfaces, plus read-only commands on the telnet shell. shell() refuses everything
that could write the FPGA flash, the calibration or the boot partitions, so a typo here cannot.
"""

import argparse, base64, json, math, os, random, re, signal, socket, statistics, struct, subprocess, sys, time, traceback, urllib.error, urllib.request

CH = (1, 2, 3, 4)
VSCALES = [1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0]
# a shell command with one of these never leaves this program
FORBIDDEN = re.compile(r"spi2|mtd|flash_|>\s*/rigol|\b(rm|mv|cp|chmod|touch|tee)\b.*/rigol|((?<![0-9])>|of=)\s*/dev/|mmcblk|by-name|\bdd\b|mkfs|uboot|\btrust\b|parameter|oos-hw\s+wr|oos-fpgad\s+wr|cold-boot|\breboot\b|poweroff", re.I)


class Gone(Exception):
    """The scope stopped answering: nothing after this is worth running."""


class Skip(Exception):
    pass


class Info(Exception):
    pass


class Scope:
    def __init__(self, host, scpi=5555, web=8080, sim=False):
        self.host, self.port, self.web, self.sim = host, scpi, web, sim
        self.s = None
        self.loop = []  # channels the generator reaches
        self.connect()

    def connect(self):
        for _ in range(30):
            try:
                self.s = socket.create_connection((self.host, self.port), timeout=10)
                self.s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                return
            except OSError:
                time.sleep(2)
        raise Gone(f"no SCPI on {self.host}:{self.port}")

    def w(self, cmd):
        try:
            self.s.sendall(cmd.encode() + b"\n")
        except OSError as e:
            raise Gone(f"SCPI write: {e}")

    def line(self, timeout=15):
        self.s.settimeout(timeout)
        b = b""
        try:
            while not b.endswith(b"\n"):
                c = self.s.recv(65536)
                if not c:
                    raise Gone("SCPI connection closed")
                b += c
        except socket.timeout:
            raise Gone(f"no SCPI answer in {timeout} s")
        except OSError as e:
            raise Gone(f"SCPI read: {e}")
        return b.decode(errors="replace").strip()

    def q(self, cmd, timeout=15):
        self.w(cmd)
        return self.line(timeout)

    def num(self, cmd):
        v = float(self.q(cmd))
        return math.nan if abs(v) > 9e37 else v

    def block(self, cmd, timeout=60):
        """IEEE 488.2 definite block."""
        self.w(cmd)
        self.s.settimeout(timeout)

        def need(n):
            b = b""
            while len(b) < n:
                c = self.s.recv(min(1 << 20, n - len(b)))
                if not c:
                    raise Gone("SCPI connection closed")
                b += c
            return b

        try:
            h = need(2)
            if h[:1] != b"#":
                raise AssertionError(f"no block: {h!r}{self.line()}")
            n = int(need(h[1] - 48))
            d = need(n)
            need(1)
        except socket.timeout:
            raise Gone(f"no block in {timeout} s")
        return d

    def set(self, **kv):
        for k, v in kv.items():
            self.w(f":SYST:SET {k.replace('__', '.')} = {v}")

    def errors(self):
        out = []
        for _ in range(25):
            e = self.q(":SYST:ERR?")
            if e.startswith("0,"):
                break
            out.append(e)
        return out

    def stat(self):
        t = self.q(":SYST:STAT?")
        m = re.search(r"records (\d+) run (\w+) status (\w+)", t)
        if not m:
            raise AssertionError(f"state line: {t}")
        f = re.search(r"fps ([\d.]+)", t)
        return {"frames": int(m[1]), "run": m[2], "status": m[3], "fps": float(f[1]) if f else 0.0}

    def tdiv(self):
        return self.num(":TIM:SCAL?")

    def settle(self, n=3, extra=0.0):
        """New records since the last change, or give up quietly: a test then fails on its values."""
        time.sleep(0.3 + extra)
        a = self.stat()["frames"]
        end = time.time() + min(30, 3 + 30 * self.tdiv() * n)
        while time.time() < end and self.stat()["frames"] < a + n:
            time.sleep(0.05)

    def meas(self, item, c=1, n=5):
        """Median of a few records."""
        v = []
        for _ in range(n):
            x = self.num(f":MEAS:ITEM? {item},CHAN{c}")
            if not math.isnan(x):
                v.append(x)
            time.sleep(0.06)
        return statistics.median(v) if v else math.nan

    def wave(self, c=1):
        """(t0, dt, volts) of the record on the screen."""
        self.w(f":WAV:SOUR CHAN{c}")
        self.w(":WAV:FORM WORD")
        p = self.q(":WAV:PRE?").split(",")
        pts, dt, t0, yinc, yorg, yref = int(p[2]), float(p[4]), float(p[5]), float(p[7]), float(p[8]), float(p[9])
        d = self.block(":WAV:DATA?")
        w = struct.unpack(f"<{len(d) // 2}H", d[: len(d) // 2 * 2])
        return t0, dt, [(x - yref) * yinc - yorg * yinc for x in w]

    def sine(self, c, f):
        """Peak-to-peak of the frequency f in the record: other things on the line do not count, as in VAMP."""
        t0, dt, v = self.wave(c)
        n = int(int(len(v) * dt * f) / (f * dt)) or len(v)
        w = 2 * math.pi * f * dt
        a = sum(x * math.sin(w * k) for k, x in enumerate(v[:n])) * 2 / n
        b = sum(x * math.cos(w * k) for k, x in enumerate(v[:n])) * 2 / n
        return 2 * math.hypot(a, b)

    def http(self, path, data=None, headers=None, timeout=30):
        r = urllib.request.Request(f"http://{self.host}:{self.web}{path}", data=data, headers=headers or {})
        try:
            with urllib.request.urlopen(r, timeout=timeout) as x:
                return x.status, x.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def cmd(self, c):
        s, b = self.http("/cmd?c=" + urllib.parse.quote(c))
        assert s == 200, f"{c}: {s} {b[:60]}"

    def shot(self, name, out):
        s, b = self.http("/screen.png")
        if s == 200:
            with open(os.path.join(out, f"{name}.png"), "wb") as f:
                f.write(b)

    def shell(self, cmd, wait=2.0, reads_card=False):
        """A read-only command on the telnet shell, the output as text."""
        if self.sim:
            raise Skip("no shell in the simulation")
        if FORBIDDEN.search(cmd) and not reads_card:
            raise AssertionError(f"refused here: {cmd}")
        s = socket.create_connection((self.host, 23), timeout=10)
        time.sleep(1)
        s.sendall(b"echo @@S; " + cmd.encode() + b"; echo @@E\n")
        s.settimeout(wait + 8)
        b, end = b"", time.time() + wait + 8
        while b.count(b"@@E") < 2 and time.time() < end:
            try:
                c = s.recv(65536)
            except socket.timeout:
                break
            if not c:
                break
            b += c
        s.close()
        t = b.decode(errors="replace")
        return t.split("@@S", 2)[-1].split("@@E")[0].strip()

    def base(self):
        """Back to a known state: defaults, the waveform view, the generator's channel at 1 V/div, generator off, running."""
        main = self.loop[0] if self.loop else 1
        self.w(":VIEW SCOP")
        self.w("*RST")
        self.w("*CLS")
        self.w(":SOUR:OUTP OFF")
        self.set(fine_v="false", fine_t="false", tfine=1)
        for c in CH:
            self.w(f":CHAN{c}:PROB 1")
            self.w(f":CHAN{c}:INV 0")
            self.w(f":CHAN{c}:COUP DC")
            self.w(f":CHAN{c}:OFFS 0")
            self.w(f":CHAN{c}:DISP {1 if c == main else 0}")
        self.w(f":CHAN{main}:SCAL 1")
        self.w(":TIM:SCAL 0.0002")
        self.w(":TIM:OFFS 0")
        self.w(":TRIG:SWE AUTO")
        self.w(f":TRIG:EDGE:SOUR CHAN{main}")
        self.w(":TRIG:EDGE:LEV 0")
        self.w(":TRIG:EDGE:SLOP POS")
        self.w(":RUN")

    def gen(self, wave="SIN", freq=1e3, vpp=2.0, off=0.0, duty=None):
        self.w(f":SOUR:FUNC {wave}")
        if duty is not None:
            self.w(f":SOUR:FUNC:SQU:DUTY {duty}")
        self.w(f":SOUR:FREQ {freq}")
        self.w(f":SOUR:VOLT {vpp}")
        self.w(f":SOUR:VOLT:OFFS {off}")
        self.w(":SOUR:OUTP ON")

    def fires(self, wait=1.5, **kv):
        """Does this trigger set-up fire on the signal? NORMAL sweep, new records with a trigger."""
        self.w(":TRIG:SWE NORM")
        self.set(**kv)
        time.sleep(0.6)
        a = self.stat()["frames"]
        time.sleep(wait)
        return self.stat()["frames"] - a


# loader, vendor storage, parameter, uboot, trust; and the calibration. Read only, and the same before and after.
FINGERPRINT = "head -c 16777216 /dev/mmcblk0 | md5sum; find /rigol/data -type f | sort | xargs md5sum | md5sum"


def fingerprint(s):
    t = s.shell(FINGERPRINT, wait=20, reads_card=True)
    h = re.findall(r"\b[0-9a-f]{32}\b", t)
    assert len(h) == 2, t
    return h


TESTS = []


def test(tier, need=()):
    def deco(f):
        TESTS.append((tier, f.__name__, f, need))
        return f

    return deco


def near(name, got, want, rel=0.03, ab=0.0):
    ok = not math.isnan(got) and abs(got - want) <= max(abs(want) * rel, ab)
    assert ok, f"{name}: {got:.6g}, expected {want:.6g} (+-{max(abs(want) * rel, ab):.3g})"


def clean(s):
    e = s.errors()
    assert not e, f"SCPI errors: {e[:3]}"


# ---------------------------------------------------------------------------------------------- a0: no cable


@test("a0")
def identity(s, o):
    p = s.q("*IDN?").split(",")
    assert p[0] == "OOS" and len(p) == 4, p
    assert re.match(r"DHO[89]\d\d", p[1]), p
    if not s.sim:
        assert p[2] not in ("", "0"), f"no serial: {p}"
    clean(s)
    return ",".join(p)


@test("a0")
def protected_areas_before(s, o):
    """Fingerprint of what must never change. With --baseline, also against an earlier run."""
    s.finger = fingerprint(s)
    if o.baseline:
        b = json.load(open(o.baseline))
        assert b == s.finger, f"changed since the baseline: {b} {s.finger}"
    with open(os.path.join(o.out, "fingerprint.json"), "w") as f:
        json.dump(s.finger, f)
    return " ".join(s.finger)


@test("a0")
def build_on_the_unit(s, o):
    t = s.shell("cat /proc/version; oos-update status 2>&1 | head -12; uptime; df -k /oos | tail -1; pidof oos-ui")
    return t


@test("a0")
def log_has_no_panic(s, o):
    t = s.shell("grep -c -i 'panic\\|segfault\\|oom' /tmp/oos-ui.log; dmesg | grep -c 'Oops:\\|Kernel panic\\|BUG:'")
    assert t.split() == ["0", "0"], t


@test("a0")
def plan_matches_this_build(s, o):
    """The same register sequences on the unit as on this computer."""
    if s.sim or not o.plan:
        raise Skip("needs --plan with the host's plan-matrix output and plan-matrix.sh on the unit")
    t = s.shell("F=/tmp/oos-fpgad.new; [ -x $F ] || F=/bin/oos-fpgad; OOS_CAL_DIR=/rigol/data sh /tmp/plan-matrix.sh $F | md5sum", wait=60)
    want = subprocess.run(["md5"], input=open(o.plan, "rb").read(), capture_output=True).stdout.decode().strip()
    assert want in t, f"unit {t}, host {want}"


@test("a0")
def depth_and_rate_matrix(s, o):
    bad = []
    n = 0
    for mask in ((1,), (1, 2), (1, 2, 3), (1, 2, 3, 4)):
        for c in CH:
            s.w(f":CHAN{c}:DISP {1 if c in mask else 0}")
        for tdiv in (5e-9, 1e-6, 2e-4, 1e-3):
            s.w(f":TIM:SCAL {tdiv}")
            for depth in ("AUTO", "1000", "10000", "100000", "1000000", "10000000"):
                s.w(f":ACQ:MDEP {depth}")
                s.settle(2)
                a = s.stat()["frames"]
                r, d = s.num(":ACQ:SRAT?"), s.num(":ACQ:MDEP?")
                n += 1
                if not (0 < r <= 1.25e9 and d > 0):
                    bad.append((mask, tdiv, depth, r, d))
                time.sleep(0.5 + min(2.0, d / 1e7))
                if s.stat()["frames"] == a and depth != "10000000":
                    bad.append((mask, tdiv, depth, "no new record"))
    s.w(":ACQ:MDEP AUTO")
    assert not bad, f"{len(bad)} of {n}: {bad[:6]}"
    clean(s)
    return f"{n} combinations"


@test("a0")
def every_time_base_runs(s, o):
    bad = []
    for e in range(-9, 1):
        for m in (1, 2, 5):
            t = m * 10.0**e
            s.w(f":TIM:SCAL {t}")
            time.sleep(0.2 + min(2.0, t * 3))
            a = s.stat()["frames"]
            # a record takes about 25 time-base units here (1.25 s at 50 ms/div)
            time.sleep(0.4 + min(3.0, t * 30))
            got = s.num(":TIM:SCAL?")
            if abs(got / t - 1) > 1e-3:
                bad.append((t, "scale", got))
            if s.stat()["frames"] == a:
                bad.append((t, "no frame"))
    assert not bad, bad[:6]
    clean(s)


@test("a0")
def zero_of_every_scale(s, o):
    """An open input reads near 0 on every scale and channel: a wrong calibration row shows here."""
    bad = []
    worst = 0.0
    # a cable on the generator is no open input
    s.gen("SIN", 1e3, 4.0)
    for c in CH:
        s.w(f":CHAN{c}:DISP 1")
        s.w(f":CHAN{c}:SCAL 1")
    s.settle(extra=0.5)
    used = [c for c in CH if s.meas("VPP", c, 3) < 3.0 and not (o.rf and c == o.rf_ch) and c not in o.busy]
    s.w(":SOUR:OUTP OFF")
    for c in CH:
        s.w(f":CHAN{c}:DISP {1 if c in used else 0}")
    for c in used:
        for v in VSCALES:
            s.w(f":CHAN{c}:SCAL {v}")
        for v in VSCALES:
            s.w(f":CHAN{c}:SCAL {v}")
            s.settle(2)
            m = s.meas("VAVG", c, 3)
            worst = max(worst, abs(m) / v)
            if not abs(m) / v < 0.25:
                bad.append((c, v, m))
    assert not bad, bad[:6]
    return f"worst zero {worst:.3f} div"


@test("a0")
def gnd_is_flat(s, o):
    s.w(":CHAN1:COUP GND")
    s.settle()
    t0, dt, v = s.wave(1)
    assert v, "no samples"
    near("mean", statistics.fmean(v), 0, ab=0.05)
    assert statistics.pstdev(v) < 0.02, statistics.pstdev(v)


@test("a0")
def normal_and_single_wait_for_a_trigger(s, o):
    s.w(":CHAN1:SCAL 1")
    s.w(":TRIG:EDGE:LEV 3.5")
    s.w(":TRIG:SWE NORM")
    time.sleep(1)
    a = s.stat()["frames"]
    time.sleep(2)
    assert s.stat()["frames"] == a, "NORMAL drew records without a trigger"
    s.w(":SING")
    time.sleep(2.5)
    st = s.stat()
    assert st["run"] == "Run", f"SINGLE ended without a trigger: {st}"
    s.w(":STOP")
    s.w(":TRIG:SWE AUTO")
    s.w(":RUN")
    s.settle()
    assert s.stat()["status"] in ("Auto", "Triggered"), s.stat()


@test("a0")
def settings_survive_the_setup_keys(s, o):
    """The test access itself: what is set reads back."""
    for k, v in (("trig_type", "Pulse"), ("holdoff", "0.001"), ("ch1.bw", None)):
        if v:
            s.w(f":SYST:SET {k} = {v}")
            assert s.q(f":SYST:GET? {k}") == v, k
    s.w(":SYST:SET nonsense = 1")
    assert any("nonsense" in e for e in s.errors()), "an unknown key went unnoticed"


@test("a0")
def web_pages_and_downloads(s, o):
    for p, want in (("/", b"screen.png"), ("/update", b"update"), ("/lxi/identification", b"Manufacturer"), ("/screen.png", b"PNG"), ("/wave.csv", b"Time")):
        c, b = s.http(p)
        assert c == 200 and want.lower() in b[:4000].lower(), (p, c, b[:80])
    c, b = s.http("/screen.bmp")
    assert c == 200 and b[:2] == b"BM"
    s.cmd("key measure")
    time.sleep(0.3)
    s.cmd("key measure")


@test("a0")
def web_keeps_foreign_sites_out(s, o):
    bad = {"Origin": "https://evil.example"}
    c, _ = s.http("/cmd?c=key%20runstop", headers=bad)
    assert c == 403, c
    c, _ = s.http("/cmd?c=key%20runstop", headers={"Sec-Fetch-Site": "cross-site"})
    assert c == 403, c
    c, _ = s.http("/wave.csv", headers={"Host": "evil.example:8080"})
    assert c == 403, c
    c, _ = s.http("/update", data=b"x", headers=bad)
    assert c == 403, c
    c, _ = s.http("/", headers={"Sec-Fetch-Site": "cross-site"})
    assert c == 200, "a link to the page must work"
    assert s.stat()["run"] == "Run", "a refused request stopped the scope"


@test("a0")
def mcp_tools(s, o):
    def rpc(method, params=None):
        c, b = s.http("/mcp", data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}).encode(), headers={"Content-Type": "application/json"})
        assert c == 200, (c, b[:100])
        return json.loads(b)

    assert {t["name"] for t in rpc("tools/list")["result"]["tools"]} >= {"scpi", "screenshot"}
    r = rpc("tools/call", {"name": "scpi", "arguments": {"command": "*IDN?"}})
    assert "OOS" in json.dumps(r), r


@test("a0")
def vxi11_answers(s, o):
    """create_link and *IDN? by hand: no VISA needed."""

    def call(k, xid, proc, args):
        m = struct.pack(">IIIIII", xid, 0, 2, 0x0607AF, 1, proc) + b"\0" * 16 + args
        k.sendall(struct.pack(">I", len(m) | 1 << 31) + m)
        h = k.recv(4)
        n = struct.unpack(">I", h)[0] & 0x7FFFFFFF
        b = b""
        while len(b) < n:
            b += k.recv(n - len(b))
        return b[24:]

    try:
        k = socket.create_connection((s.host, 9009), timeout=5)
    except OSError as e:
        if s.sim:
            raise Skip(f"VXI-11 port: {e}")
        raise AssertionError(f"VXI-11 port: {e}")
    k.settimeout(5)
    name = b"inst0\0\0\0"
    r = call(k, 1, 10, struct.pack(">IIII", 1, 0, 0, 5) + name)
    err, lid = struct.unpack(">II", r[:8])
    assert err == 0, err
    cmd = b"*IDN?\n"
    call(k, 2, 11, struct.pack(">IIII", lid, 1000, 1000, 8) + struct.pack(">I", len(cmd)) + cmd + b"\0\0")
    r = call(k, 3, 12, struct.pack(">IIIIII", lid, 1000, 1000, 0, 0, 0))
    assert b"OOS" in r, r[:60]
    call(k, 4, 23, struct.pack(">I", lid))
    k.close()


@test("a0")
def limits_hold(s, o):
    """Nonsense in quantity leaves the scope alive and answering."""
    for _ in range(200):
        s.w(":NOPE" + "x" * 5000)
    e = s.errors()
    assert len(e) <= 21 and all(len(x) < 400 for x in e), len(e)
    extra = []
    for _ in range(12):
        try:
            extra.append(socket.create_connection((s.host, s.port), timeout=3))
        except OSError:
            pass
    time.sleep(0.5)
    for k in extra:
        k.close()
    t = time.time()
    s.http("/cmd?c=knob%20hpos%202000000000")
    assert time.time() - t < 5, "a remote knob turn stopped the frame loop"
    s.w(":TIM:OFFS 0")
    assert "OOS" in s.q("*IDN?")
    for bad in ("/cmd?c=quit", "/cmd?c=shot%20/tmp/x", "/nothing"):
        c, _ = s.http(bad)
        assert c in (403, 404), (bad, c)


@test("a0")
def random_settings_do_not_kill_it(s, o):
    """Random but valid settings, the kind of mix nobody tries by hand."""
    rnd = random.Random(1)
    a = s.stat()["frames"]
    keys = [
        ("trig_type", ["Edge", "Pulse", "Slope", "Pattern", "Duration", "Timeout", "Runt", "Window", "NthEdge"]),
        ("trig_mode", ["Auto", "Normal"]),
        ("slope", ["Rise", "Fall", "Either"]),
        ("mdepth", [0, 1, 2, 3, 4]),
        ("ch1.coupling", ["Dc", "Ac", "Gnd"]),
        ("ch2.on", ["true", "false"]),
        ("ch3.on", ["true", "false"]),
        ("ch4.on", ["true", "false"]),
        ("ch1.invert", ["true", "false"]),
        ("ch2.probe", [1, 10, 100]),
        ("dec_on", ["true", "false"]),
        ("dec_proto", ["Uart", "I2c", "Spi", "Can"]),
        ("persist", [0, 1, 2]),
        ("acq", ["Normal", "Peak", "Average", "HiRes"]),
    ]
    n = 300 if not o.quick else 40
    for i in range(n):
        k, vals = rnd.choice(keys)
        s.w(f":SYST:SET {k} = {rnd.choice(vals)}")
        s.w(f":TIM:SCAL {rnd.choice([1e-8, 1e-6, 1e-4, 1e-3, 1e-2])}")
        s.w(f":CHAN1:SCAL {rnd.choice(VSCALES)}")
        s.w(f":TIM:OFFS {rnd.uniform(-1, 1) * 1e-4}")
        time.sleep(0.12)
        if i % 25 == 0:
            s.q("*IDN?", timeout=30)
    left = [e for e in s.errors() if "unknown key" in e]
    assert not left, left
    s.base()
    s.settle()
    assert s.stat()["frames"] > a
    return f"{n} changes"


@test("a0")
def menus_open_and_close(s, o):
    """Every key with a page, one picture each to look at."""
    for k in ("ch1", "ch2", "tmenu", "hmenu", "measure", "cursor", "math", "ref", "analyse", "la", "gi", "navigate", "quick"):
        s.cmd(f"key {k}")
        time.sleep(0.4)
        s.shot(f"menu-{k}", o.out)
        s.cmd(f"key {k}")
        time.sleep(0.2)
    s.base()
    s.q("*IDN?")


# Frame rate: of the 30 a second the frame loop aims at, at least this many with four channels in Run, in every
# combination of graphics and monitor mode; the CPU drawing the monitor's own screen (Layout) is known to be heavier.
FPS_FLOOR = 27.0
FPS_FLOOR_CPU_LAYOUT = 22.0
# the GPU may be this far below the CPU in the same mode (noise), and its frame loop this much busier (points)
GPU_BEHIND_FPS = 1.5
GPU_BUSIER = 5.0
# The GPU's own time (the helper's upload and draw, `gpu panel .. monitor ..` of the state line): a monitor frame comes
# with every second panel frame, so the pair has 2/30 s; two thirds of that, and of one frame without a monitor. The
# frame rate alone hides a GPU near its limit: up to 0.1.11 the pair took about 56 ms with a monitor and the panel
# still made 30 frames a second, until a menu or colour grade tipped it over.
GPU_PAIR_MS = 45.0
GPU_PANEL_MS = 22.0


def frame_rate(s, secs=4.0):
    """Frames a second over `secs` (the frame counter, not the smoothed value) and the last state line."""
    t0, a = time.time(), s.q(":SYST:STAT?")
    time.sleep(secs)
    b = s.q(":SYST:STAT?")
    n = lambda t: int(re.search(r"frames (\d+)", t)[1])
    return (n(b) - n(a)) / (time.time() - t0), b


def loop_load(s, secs=3):
    """Percent of a core: the busiest thread of oos-ui (the frame loop) and the GPU helper in all."""
    snap = "for p in $(pidof oos-ui) $(pidof oos-gpu); do for t in /proc/$p/task/*; do echo $p $(cut -d' ' -f1,2,14,15 $t/stat); done; done"
    t = s.shell(f"{snap}; echo @@M; sleep {secs}; {snap}", wait=secs + 2)
    a, _, b = t.partition("@@M")
    ticks = lambda x: {l.split()[1]: (l.split()[0], l.split()[2], int(l.split()[3]) + int(l.split()[4])) for l in x.strip().splitlines() if len(l.split()) == 5}
    a, b = ticks(a), ticks(b)
    ui_pid = s.shell("pidof oos-ui").strip()
    loop, helper = 0.0, 0.0
    for tid, (pid, name, v) in b.items():
        d = (v - a.get(tid, (pid, name, v))[2]) / secs  # ticks of 10 ms: percent
        if pid == ui_pid:
            loop = max(loop, d)
        else:
            helper += d
    return loop, helper


@test("a0")
def frame_rate_by_graphics_and_monitor(s, o):
    """Four channels in Run, the frame rate and the frame loop's load with System > Graphics = CPU and = GPU, and with a
    monitor plugged in, in each HDMI mode. The GPU must not fall behind the CPU: with a monitor it did up to 0.1.11
    (25 frames a second against 30) and no test saw it."""
    if s.sim:
        raise Skip("no frame loop of the scope in the simulation")
    keep_g, keep_h = s.q(":SYST:GRAP?"), s.q(":SYST:GET? hdmi")
    s.base()
    for c in CH:
        s.w(f":CHAN{c}:DISP 1")
        s.w(f":CHAN{c}:SCAL 1")
    s.w(":TIM:SCAL 0.0005")
    mon = re.search(r"hdmi (\d+x\d+)", s.q(":SYST:STAT?"))
    modes = ["Mirror", "Waveform", "Layout"] if mon else [None]
    rows, bad = {}, []
    # with a menu open, as the screen mostly is while someone works at it
    s.cmd("key measure")
    try:
        for g in ("CPU", "GPU"):
            s.w(f":SYST:GRAP {g}")
            time.sleep(3)  # the GPU helper starts
            for h in modes:
                if h:
                    s.w(f":SYST:SET hdmi = {h}")
                time.sleep(2)
                fps, st = frame_rate(s)
                loop, helper = loop_load(s)
                if g == "GPU" and " gpu layers" not in st:
                    bad.append(f"GPU {h or 'panel'}: Graphics = GPU, but the CPU draws (the helper did not start or failed)")
                gp = re.search(r"gpu panel ([\d.]+)(?: monitor ([\d.]+))? ms", st)
                pan = float(gp[1]) if gp else math.nan
                mo = float(gp[2]) if gp and gp[2] else (math.nan if h else 0.0)
                rows[(g, h)] = (fps, loop, helper, pan, mo)
                if g == "GPU":
                    if h and not pan + mo <= GPU_PAIR_MS:
                        bad.append(f"GPU {h}: panel {pan:.1f} + monitor {mo:.1f} ms of GPU time, at most {GPU_PAIR_MS:.0f} for the pair")
                    if not h and not pan <= GPU_PANEL_MS:
                        bad.append(f"GPU panel: {pan:.1f} ms of GPU time a frame, at most {GPU_PANEL_MS:.0f}")
    finally:
        s.cmd("key measure")
        s.w(f":SYST:GRAP {keep_g}")
        s.w(f":SYST:SET hdmi = {keep_h}")
        s.base()
    for (g, h), (fps, loop, *_) in rows.items():
        floor = FPS_FLOOR_CPU_LAYOUT if (g, h) == ("CPU", "Layout") else FPS_FLOOR
        if fps < floor:
            bad.append(f"{g} {h or 'panel'}: {fps:.1f} frames a second, at least {floor:.0f} expected")
    for h in modes:
        c, gp = rows[("CPU", h)], rows[("GPU", h)]
        if gp[0] < c[0] - GPU_BEHIND_FPS:
            bad.append(f"{h or 'panel'}: the GPU {gp[0]:.1f} frames a second, the CPU {c[0]:.1f}")
        if gp[1] > c[1] + GPU_BUSIER:
            bad.append(f"{h or 'panel'}: the frame loop busier with the GPU ({gp[1]:.0f} %) than with the CPU ({c[1]:.0f} %)")
    table = "; ".join(f"{g} {h or 'panel'} {v[0]:.1f} fps loop {v[1]:.0f} %" + (f" helper {v[2]:.0f} % gpu {v[3]:.1f}" + (f"+{v[4]:.1f}" if h else "") + " ms" if g == "GPU" else "") for (g, h), v in rows.items())
    assert not bad, " / ".join(bad) + " -- " + table
    return (f"monitor {mon[1]}: " if mon else "no monitor: ") + table


# ---------------------------------------------------------------------------------------------- a1: generator on CH1


@test("a1")
def generator_reaches(s, o):
    """Which channels see the generator: everything after this depends on it."""
    for c in CH:
        s.w(f":CHAN{c}:DISP 1")
        s.w(f":CHAN{c}:SCAL 1")
    s.gen("SIN", 1e3, 4.0)
    s.settle(extra=0.5)
    s.loop = [c for c in CH if s.meas("VPP", c, 3) > 3.0]
    s.w(":SOUR:OUTP OFF")
    if not s.loop:
        raise Skip("the generator reaches no channel: no cable")
    return f"generator on CH{s.loop}"


@test("a1", need=(1,))
def generator_frequency_and_amplitude(s, o):
    s.w(f":CHAN{s.loop[0]}:BWL AUTO")  # levels, not bandwidth: Full reads the noise peaks too
    bad = []
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    for f in (1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 2e7):
        s.w(f":TIM:SCAL {max(5e-9, 1 / f / 2)}")
        s.gen("SIN", f, 2.0)
        s.settle(extra=0.2)
        fr, vp = s.meas("FREQ", c), s.sine(c, f)  # the fit: a single spike on the cable is no amplitude
        if not abs(fr / f - 1) < 0.01:
            bad.append((f, "freq", fr))
        # above 1 MHz this is the open cable as much as the scope, a 50 Ohm through in part C says more
        if not abs(vp / 2 - 1) < (0.05 if f <= 1e6 else 0.3):
            bad.append((f, "vpp", vp))
    assert not bad, bad


@test("a1", need=(1,))
def every_scale_reads_true(s, o):
    """A sine of 4 div on each scale: a wrong calibration row, as 100 and 200 mV had, is 2 % off here."""
    s.w(f":CHAN{s.loop[0]}:BWL AUTO")  # levels, not bandwidth: Full reads the noise peaks too
    c = s.loop[0]
    bad = []
    rows = []
    for v in VSCALES:
        amp = min(4 * v, 10.0)
        s.w(":TIM:SCAL 0.0002")
        s.gen("SIN", 1e3, amp)
        # the generator's own few mV of DC put the lowest scales off the screen: the channel offset brings it back
        s.w(f":CHAN{c}:OFFS 0")
        if v < 0.005:
            s.w(f":CHAN{c}:SCAL 0.01")
            s.settle(extra=0.5)
            s.w(f":CHAN{c}:OFFS {-s.meas('VAVG', c)}")
        s.w(f":CHAN{c}:SCAL {v}")
        s.settle(extra=0.2)
        got = statistics.median(s.sine(c, 1e3) for _ in range(3))
        err = got / amp - 1
        rows.append(f"{v:g}:{err * 100:+.1f}%")
        # below 10 mV/div the generator itself is at its low end
        if not abs(err) < (0.03 if v >= 0.01 else 0.15):
            bad.append((v, got))
    s.w(f":CHAN{c}:OFFS 0")
    assert not bad, f"{bad} of {rows}"
    return " ".join(rows)


@test("a1", need=(1,))
def fine_scale_keeps_the_reading(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e3, 2.0)
    s.w(f":CHAN{c}:SCAL 1")
    ref = None
    for fine in (1.0, 1.1, 1.3, 1.5, 1.8, 1.96):
        s.set(**{f"ch{c}__vfine": fine})
        s.settle(extra=0.2)
        got = s.meas("VAMP", c)
        ref = ref or got
        near(f"at {fine}", got, ref, rel=0.02)
    s.set(**{f"ch{c}__vfine": 1})


@test("a1", need=(1,))
def offset_moves_the_trace_not_the_reading(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    s.gen("DC", 1e3, 1.0, 1.0)
    for off in (0.0, -1.0, -1.5, 0.5):
        s.w(f":CHAN{c}:OFFS {off}")
        s.settle(extra=0.2)
        near(f"offset {off}", s.meas("VAVG", c), 1.0, rel=0.03, ab=0.03)
    s.w(f":CHAN{c}:OFFS 0")


@test("a1", need=(1,))
def offset_stays_in_the_scale_range(s, o):
    c = s.loop[0]
    # a level the offset brings back onto the screen; the wrapped DAC of before threw it far off
    s.gen("DC", 1e3, 1.0, -3.0)
    s.w(f":CHAN{c}:SCAL 0.1")
    s.w(f":CHAN{c}:OFFS 3")
    s.settle(extra=0.2)
    near("offset at 100 mV", s.num(f":CHAN{c}:OFFS?"), 3.0, ab=0.01)
    near("level at 100 mV", s.meas("VAVG", c), -3.0, rel=0.03)
    s.w(":SOUR:VOLT:OFFS -2")
    s.w(f":CHAN{c}:SCAL 0.05")
    s.settle(extra=0.3)
    near("clamped at 50 mV", s.num(f":CHAN{c}:OFFS?"), 2.0, ab=0.01)
    near("level at 50 mV", s.meas("VAVG", c), -2.0, rel=0.03)
    s.w(f":CHAN{c}:OFFS 0")


@test("a1", need=(1,))
def probe_ratio_scales_everything(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.gen("SIN", 1e3, 2.0)
    s.settle(extra=0.2)
    a = s.meas("VAMP", c)
    s.w(f":CHAN{c}:PROB 10")
    s.w(f":CHAN{c}:SCAL 10")
    s.w(":TRIG:EDGE:LEV 0")
    s.settle(extra=0.3)
    near("reading at 10x", s.meas("VAMP", c), 10 * a, rel=0.02)
    # the trace fills the same part of the screen as at 1x
    s.w(f":CHAN{c}:PROB 1")
    s.w(f":CHAN{c}:SCAL 1")


@test("a1", need=(1,))
def ac_coupling_blocks_dc(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e4, 0.4, 2.0)
    s.w(f":CHAN{c}:SCAL 0.05")
    s.w(":TIM:SCAL 0.00005")
    s.w(f":CHAN{c}:COUP AC")
    s.settle(extra=1.5)
    near("mean", s.meas("VAVG", c), 0, ab=0.03)
    near("ripple through", s.sine(c, 1e4), 0.4, rel=0.05)
    s.w(f":CHAN{c}:COUP DC")
    s.w(f":CHAN{c}:SCAL 1")
    s.settle(extra=0.5)
    near("DC back", s.meas("VAVG", c), 2.0, rel=0.03, ab=0.05)


@test("a1", need=(1,))
def invert_mirrors_trace_and_offset(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    s.gen("DC", 1e3, 1.0, 1.0)
    s.w(f":CHAN{c}:INV 1")
    s.settle(extra=0.3)
    near("inverted", s.meas("VAVG", c), -1.0, rel=0.03, ab=0.03)
    s.w(f":CHAN{c}:OFFS 1")
    s.settle(extra=0.3)
    near("with offset", s.meas("VAVG", c), -1.0, rel=0.03, ab=0.03)
    s.w(f":CHAN{c}:OFFS 0")
    s.w(f":CHAN{c}:INV 0")
    s.settle(extra=0.3)
    near("back", s.meas("VAVG", c), 1.0, rel=0.03, ab=0.03)


@test("a1", need=(1,))
def invert_and_the_trigger(s, o):
    """Open question of the docs: the trigger before or after the invert. The answer goes into the report."""
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.gen("SQU", 1e3, 3.0, 1.5)
    s.w(f":CHAN{c}:INV 1")
    res = {}
    for lv in (-1.5, 1.5):
        s.w(f":TRIG:EDGE:LEV {lv}")
        res[lv] = s.fires()
    s.w(f":CHAN{c}:INV 0")
    s.w(":TRIG:SWE AUTO")
    after = res[-1.5] > 0
    raise Info(f"inverted 0..3 V: level -1.5 fires {res[-1.5]}, level +1.5 fires {res[1.5]}: the trigger sees the signal {'after' if after else 'before'} the invert")


@test("a1", need=(1,))
def delay_moves_the_record(s, o):
    """The whole screen holds data at any delay, and the edge sits where the delay says."""
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.gen("SQU", 2e5, 2.0)
    s.w(":TIM:SCAL 0.000001")
    s.w(":TRIG:SWE NORM")
    for d in (0, 3e-6, -3e-6, 4.5e-6):
        s.w(f":TIM:OFFS {d}")
        s.settle(extra=0.3)
        t0, dt, v = s.wave(c)
        assert len(v) > 100
        blank = sum(1 for x in v if not math.isfinite(x) or abs(x) > 50) / len(v)
        assert blank < 0.02, f"delay {d}: {blank:.0%} of the screen empty"
        near(f"window at {d}", t0 + dt * len(v) / 2, d, ab=2e-7)
        if t0 + 3e-7 < 0 < t0 + dt * len(v) - 3e-7:
            i, k = int(-t0 / dt), int(2e-7 / dt)
            assert v[i - k] < 0 < v[i + k], f"delay {d}: no rising edge at t = 0 ({v[i - k]:.2f}, {v[i + k]:.2f})"
    s.w(":TIM:OFFS 0")
    s.w(":TRIG:SWE AUTO")


@test("a1", need=(1,))
def fine_time_base_shows_the_right_time(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e4, 2.0)
    s.w(":TIM:SCAL 0.0002")
    s.set(tfine=0.75)
    s.settle(extra=0.3)
    near("scale", s.tdiv(), 150e-6, rel=0.001)
    near("frequency", s.meas("FREQ", c), 1e4, rel=0.01)
    t0, dt, v = s.wave(c)
    near("window", dt * len(v), 1.5e-3, rel=0.02)
    s.set(tfine=1)


@test("a1", need=(1,))
def edge_trigger_on_level_and_slope(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.gen("SIN", 1e3, 4.0)
    for slope, sign in (("POS", 1), ("NEG", -1)):
        for lv in (-1.0, 0.0, 1.0):
            s.w(f":TRIG:EDGE:LEV {lv}")
            s.w(f":TRIG:EDGE:SLOP {slope}")
            s.settle(extra=0.3)
            t0, dt, v = s.wave(c)
            i = int(round(-t0 / dt))
            near(f"level {lv} {slope}", v[i], lv, ab=0.2)
            k = max(1, int(5e-5 / dt))
            assert (v[i + k] - v[i - k]) * sign > 0, f"{slope}: wrong edge"
    s.w(":TRIG:EDGE:SLOP POS")
    s.w(":TRIG:EDGE:LEV 0")


@test("a1", need=(1,))
def trigger_types_fire_and_hold_back(s, o):
    """Each type on a signal that meets it and on one that does not."""
    c = s.loop[0]
    src = c - 1
    s.w(f":CHAN{c}:SCAL 1")
    s.w(":TIM:SCAL 0.00005")
    s.set(trig_src=src, trig_level=0)
    s.gen("SQU", 1e4, 2.0, duty=50)  # 50 us high, 50 us low
    bad = []

    def both(name, yes, no):
        a, b = s.fires(**yes), s.fires(**no)
        if a == 0 or b != 0:
            bad.append(f"{name}: fires {a} and {b}")

    both("pulse", dict(trig_type="Pulse", trig_pol="Pos", trig_when="Gt", trig_t1=40e-6), dict(trig_t1=60e-6))
    both("pulse<", dict(trig_when="Lt", trig_t1=60e-6), dict(trig_t1=40e-6))
    both("timeout", dict(trig_type="Timeout", trig_t1=30e-6), dict(trig_t1=120e-6))
    both("window", dict(trig_type="Window", trig_level=0.5, trig_level2=-0.5), dict(trig_level=1.8, trig_level2=-1.8))
    s.set(trig_level=0, trig_level2=-1)
    both("first edge after idle", dict(trig_type="NthEdge", nth_n=1, nth_idle=20e-6), dict(nth_idle=1.0))
    # three pulses, then a pause: the third edge exists, the fourth does not
    s.w(":SOUR:ARB:DATA " + ",".join(str(b) for b in [1, 0, 1, 0, 1, 0] + [0] * 10 for _ in range(16)))
    s.w(":SOUR:FREQ 1000")
    s.w(":SOUR:VOLT 2")
    both("nth edge", dict(trig_level=0, nth_n=3, nth_idle=300e-6), dict(nth_n=4))
    s.gen("RAMP", 1e3, 2.0)  # 500 us per slope, +-0.5 V in 250 us
    both("slope", dict(trig_type="Slope", trig_when="Gt", trig_level=0.5, trig_level2=-0.5, trig_t1=100e-6), dict(trig_t1=400e-6))
    # a pattern fires when it comes true: a square makes it so, a level below it never
    pat = f"{' '.join('H' if i == src else 'X' for i in range(4))}"
    s.set(pat_levels="0 0 0 0")
    s.gen("SQU", 1e4, 2.0, duty=50)
    a = s.fires(trig_type="Pattern", pat=pat)
    s.gen("DC", 1e3, 1.0, -1.0)
    b = s.fires()
    if a == 0 or b != 0:
        bad.append(f"pattern: fires {a} and {b}")
    s.set(trig_type="Edge", trig_level=0)
    s.w(":TRIG:SWE AUTO")
    assert not bad, bad


@test("a1", need=(1,))
def runt_trigger(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(":TIM:SCAL 0.0001")
    s.w(":SOUR:ARB:DATA " + ",".join(["-1"] * 8 + ["1"] * 8 + ["-1"] * 8 + ["0"] * 8))
    s.w(":SOUR:FREQ 1000")
    s.w(":SOUR:VOLT 2")
    s.w(":SOUR:OUTP ON")
    a = s.fires(trig_src=c - 1, trig_type="Runt", trig_pol="Pos", trig_when="Off", trig_level=0.6, trig_level2=-0.6)
    s.set(trig_type="Edge", trig_level=0)
    s.w(":TRIG:SWE AUTO")
    assert a > 0, "no runt found"


@test("a1", need=(1,))
def single_takes_one_new_record(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.gen("SIN", 1e3, 1.0)
    s.settle()
    s.w(":STOP")
    time.sleep(1.5)
    s.w(":SOUR:VOLT 4")
    time.sleep(0.7)
    s.w(":SING")
    end = time.time() + 5
    while s.stat()["run"] == "Run" and time.time() < end:
        time.sleep(0.05)
    assert s.stat()["run"] == "Stop", "SINGLE did not end"
    near("amplitude of after the press", s.meas("VAMP", c, 1), 4.0, rel=0.05)
    a = s.stat()["frames"]
    time.sleep(1)
    assert s.stat()["run"] == "Stop"
    s.w(":TRIG:SWE AUTO")
    s.w(":RUN")


@test("a1", need=(1,))
def peak_detect_keeps_glitches(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    # 2.4 us every 10 ms at 50 kSa/s: normal mode misses most of them
    s.w(":SOUR:ARB:DATA " + ",".join(["1"] + ["0"] * 4095))
    s.w(":SOUR:FREQ 100")
    s.w(":SOUR:VOLT 2")
    s.w(":SOUR:OUTP ON")
    s.w(":TIM:SCAL 0.002")  # below the roll time bases, or the depth would be 1 M
    s.w(":ACQ:MDEP 1000")
    s.set(acq="Normal")
    s.settle(extra=0.4)
    a = s.meas("VPP", c)
    s.set(acq="Peak")
    s.settle(extra=0.4)
    b = s.meas("VPP", c)
    s.set(acq="Normal")
    s.w(":ACQ:MDEP AUTO")
    if s.sim:
        raise Skip(f"the demo source has no peak detect: normal {a:.2f} V, peak {b:.2f} V")
    # the short pulse has overshoot on the open cable, so more than 2 V is right
    assert b > 1.7 and b > a + 0.3, f"normal {a:.2f} V, peak {b:.2f} V"
    return f"normal {a:.2f} V, peak {b:.2f} V"


@test("a1", need=(1,))
def holdoff_slows_the_trigger(s, o):
    if s.sim:
        raise Skip("the demo source has no holdoff")
    c = s.loop[0]
    s.gen("SQU", 1e3, 2.0)
    s.w(":TIM:SCAL 0.00001")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.w(":TRIG:SWE NORM")
    s.settle()
    a = s.stat()["frames"]
    time.sleep(2)
    n0 = s.stat()["frames"] - a
    s.w(":TRIG:HOLD 0.5")
    clean(s)
    time.sleep(1)
    a = s.stat()["frames"]
    time.sleep(3)
    n1 = s.stat()["frames"] - a
    s.w(":TRIG:HOLD 0")
    s.w(":TRIG:SWE AUTO")
    assert s.q(":TRIG:HOLD?") and n1 < max(n0 / 2, 12), f"{n0} per 2 s, with holdoff {n1} per 3 s"
    return f"{n0} per 2 s without, {n1} per 3 s with 0.5 s"


@test("a1", need=(1,))
def bandwidth_limit_and_auto(s, o):
    """Numbers for the open question: what Auto does to 10 MHz at 200 us/div."""
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    s.gen("SIN", 1e7, 2.0)
    rows = {}
    for bw in ("OFF", "20M", "AUTO"):
        s.w(f":CHAN{c}:BWL {bw}")
        for t in (1e-7, 2e-4):
            s.w(f":TIM:SCAL {t}")
            s.settle(extra=0.4)
            rows[f"{bw} {t:g}"] = round(s.meas("VPP", c), 3)
    s.w(f":CHAN{c}:BWL AUTO")
    s.w(":TIM:SCAL 0.0002")
    s.gen("SIN", 3e7, 2.0)
    s.w(":TIM:SCAL 0.00000002")
    s.w(f":CHAN{c}:BWL OFF")
    s.settle(extra=0.4)
    full = s.meas("VPP", c)
    s.w(f":CHAN{c}:BWL 20M")
    s.settle(extra=0.4)
    lim = s.meas("VPP", c)
    s.w(f":CHAN{c}:BWL AUTO")
    assert s.sim or lim < 0.8 * full, f"20 MHz limit at 30 MHz: {full} -> {lim}"
    raise Info(f"Vpp of 10 MHz, 2 Vpp: {rows}; 30 MHz full {full:.2f}, limited {lim:.2f}")


@test("a1", need=(1,))
def generator_waves(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    s.w(":TIM:SCAL 0.0002")
    s.gen("SQU", 1e3, 2.0, duty=30)
    s.settle(extra=0.3)
    near("square top", s.meas("VTOP", c), 1.0, rel=0.05)
    near("duty", s.meas("PDUT", c), 30, ab=2.0) if not s.sim else None
    s.gen("RAMP", 1e3, 2.0)
    s.settle(extra=0.3)
    near("ramp", s.meas("VPP", c), 2.0, rel=0.05)
    s.gen("DC", 1e3, 1.0, 1.5)
    s.settle(extra=0.3)
    near("dc", s.meas("VAVG", c), 1.5, rel=0.03, ab=0.03)
    s.gen("NOIS", 1e3, 2.0)
    s.settle(extra=0.3)
    assert s.meas("VRMS", c) > 0.1
    s.w(":SOUR:ARB:DATA 0,1,0,-1")
    s.w(":SOUR:FREQ 1000")
    s.w(":SOUR:VOLT 2")
    s.settle(extra=0.3)
    near("arb", s.meas("VPP", c), 2.0, rel=0.08)
    clean(s)


@test("a1", need=(1,))
def generator_off_works_while_stopped(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e3, 2.0)
    s.settle(extra=0.2)
    assert s.meas("VPP", c) > 1.5
    s.w(":STOP")
    time.sleep(0.3)
    s.w(":SOUR:OUTP OFF")
    time.sleep(0.5)
    s.w(":RUN")
    s.settle(extra=0.3)
    assert s.meas("VPP", c) < 0.3, "the output stayed on"


@test("a1", need=(1,))
def uart_from_the_generator_decodes(s, o):
    """The generator plays a UART byte, the decoder must read it."""
    c = s.loop[0]
    bits = [1] * 4 + [0] + [(0x4F >> i) & 1 for i in range(8)] + [1] * 3
    # the table interpolates between samples: many per bit keep the edges short
    s.w(":SOUR:ARB:DATA " + ",".join(str(b) for b in bits for _ in range(16)))
    baud = 9600
    s.w(f":SOUR:FREQ {baud / len(bits)}")
    s.w(":SOUR:VOLT 3")
    s.w(":SOUR:VOLT:OFFS 1.5")
    s.w(":SOUR:OUTP ON")
    s.w(f":CHAN{c}:SCAL 1")
    # about 12 frames per record: the first of a record may begin in the middle of one and read wrong (a data 1 then a
    # 0 looks like stop and start), with 3 frames per record (0.5 ms/div) that alone came near the 60 %
    s.w(":TIM:SCAL 0.002")
    s.set(dec_on="true", dec_proto="Uart", dec_src=c - 1, dec_baud=3, dec_thr=1.5, dec_ascii="false")
    s.settle(extra=0.8)
    rows = []
    for _ in range(5):
        rows += [r.split(",") for r in s.q(":DEC:EVEN?").splitlines()]
        time.sleep(0.2)
    s.set(dec_on="false", dec_ascii="true")
    good = sum(1 for r in rows if len(r) > 3 and r[2].upper() == "4F" and r[3] == "ok")
    assert good >= len(rows) * 0.6 and good > 3, f"{good} of {len(rows)}: {rows[:6]}"
    return f"{good} of {len(rows)} frames"


def crossings(t0, dt, v):
    """Times of the rising zero crossings, interpolated."""
    out, armed = [], False
    for k in range(1, len(v)):
        armed = armed or v[k] < -0.3  # hysteresis: noise around zero is no crossing
        if armed and v[k - 1] < 0 <= v[k]:
            out.append(t0 + (k - 1 + v[k - 1] / (v[k - 1] - v[k])) * dt)
            armed = False
    return out


def sine_amplitude(t, y, f):
    """Amplitude of the frequency f in samples y at times t: least squares of a sin + b cos + c."""
    m = [[0.0] * 3 for _ in range(3)]
    r = [0.0] * 3
    for ti, yi in zip(t, y):
        v = (math.sin(2 * math.pi * f * ti), math.cos(2 * math.pi * f * ti), 1.0)
        for i in range(3):
            r[i] += v[i] * yi
            for k in range(3):
                m[i][k] += v[i] * v[k]
    for i in range(3):
        p = m[i][i]
        m[i] = [x / p for x in m[i]]
        r[i] /= p
        for k in range(3):
            if k != i:
                q = m[k][i]
                m[k] = [a - q * b for a, b in zip(m[k], m[i])]
                r[k] -= q * r[i]
    return math.hypot(r[0], r[1])



@test("a1", need=(1,))
def generator_modulation(s, o):
    """AM, FM and PM on every sample of the record (:WAVeform:MODE RAW), ten modulation periods: 50 kHz carrier, 1 kHz
    modulation. Zero crossings with hysteresis, so that noise at the crossing adds none."""
    if s.sim:
        raise Skip("the demo source has no modulation")
    c = s.loop[0]
    fc, fm = 5e4, 1e3
    s.w(f":CHAN{c}:SCAL 0.5")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.w(":TIM:SCAL 0.001")
    s.w(":ACQ:MDEP 100000")
    s.w(":WAV:MODE RAW")
    s.gen("SIN", fc, 2.0)
    res = {}

    def record():
        s.settle(n=2, extra=1.5)
        t0, dt, v = s.wave(c)
        return [t0 + k * dt for k in range(len(v))], v

    def zeros(t, v, h=0.2):
        out, armed = [], False
        for k in range(1, len(v)):
            if v[k] < -h:
                armed = True
            elif armed and v[k - 1] < 0 <= v[k]:
                out.append(t[k - 1] + (t[k] - t[k - 1]) * -v[k - 1] / (v[k] - v[k - 1]))
                armed = False
        return out

    def three(read):
        return statistics.median(read() for _ in range(3))

    def am():
        t, v = record()
        n = max(4, round(len(v) / ((t[-1] - t[0]) * fc)))
        env = [max(v[k : k + n]) for k in range(0, len(v) - n, n)]
        mid = sum(env) / len(env)
        return 100 * sine_amplitude(t[: len(env) * n : n], env, fm) / mid

    def fm_dev():
        z = zeros(*record())
        f = [(1 / (b - a), (a + b) / 2) for a, b in zip(z, z[1:])]
        return sine_amplitude([x[1] for x in f], [x[0] for x in f], fm)

    def pm_dev():
        z = zeros(*record())
        slope = (z[-1] - z[0]) / (len(z) - 1)
        return sine_amplitude(z, [(z[i] - z[0] - i * slope) * fc * 360 for i in range(len(z))], fm)

    try:
        s.w(":SOUR:MOD:TYP AM")
        s.w(":SOUR:MOD:AM:DEPT 50")
        s.w(f":SOUR:MOD:AM:INT:FREQ {fm}")
        s.w(":SOUR:MOD ON")
        res["AM %"] = three(am)
        s.w(":SOUR:MOD:TYP FM")
        s.w(f":SOUR:MOD:FM:INT:FREQ {fm}")
        for dev in (1e3, 5e3, 2e4):
            s.w(f":SOUR:MOD:FM:DEV {dev}")
            res[f"FM {dev:g}"] = three(fm_dev)
        s.w(":SOUR:MOD:TYP PM")
        s.w(f":SOUR:MOD:PM:INT:FREQ {fm}")
        for dev in (20, 90, 180):
            s.w(f":SOUR:MOD:PM:DEV {dev}")
            res[f"PM {dev}"] = three(pm_dev)
    finally:
        s.w(":SOUR:MOD OFF")
        s.w(":WAV:MODE NORM")
        s.w(":ACQ:MDEP AUTO")
    txt = ", ".join(f"{k}: {v:.1f}" for k, v in res.items())
    for k, v in res.items():
        want = 50 if k == "AM %" else float(k.split()[1])
        assert abs(v / want - 1) < 0.05, txt
    return txt


@test("a1", need=(1,))
def response_over_frequency(s, o):
    """Vpp up to the generator's limit with and without limits: figures, the open cable is part of them."""
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 0.5")
    s.w(f":CHAN{c}:BWL OFF")
    rows = {}
    for f in (1e5, 1e6, 5e6, 1e7, 2e7, 3e7, 4e7, 5e7, 6e7):
        s.gen("SIN", f, 2.0)
        s.w(f":TIM:SCAL {max(5e-9, 1 / f / 2)}")
        s.settle(extra=0.3)
        rows[f"{f / 1e6:g} MHz"] = round(s.meas("VPP", c), 3)
    s.errors()
    raise Info(str(rows))


@test("a1", need=(1,))
def deep_memory(s, o):
    """Up to 125 M points, and the data comes back."""
    c = s.loop[0]
    s.gen("SIN", 1e3, 2.0)
    s.w(":TIM:SCAL 0.0002")
    rows = []
    for d in ("10000000", "25000000", "50000000", "100000000", "125000000"):
        s.w(f":ACQ:MDEP {d}")
        time.sleep(2)
        a, t0 = s.stat()["frames"], time.time()
        while s.stat()["frames"] == a and time.time() - t0 < 60:
            time.sleep(0.2)
        got = s.stat()["frames"] > a
        # Vpp over every sample of millions at 1.25 GSa/s takes the noise peaks with it (about 2.16 V for 2.04 V of
        # sine on a DHO924S): the RMS (as Vpp of a sine) checks the data, Vpp is only reported
        v, rms = s.meas("VPP", c, 1), s.meas("VRMS", c, 1)
        rows.append(f"{int(d) // 1000000} M: {time.time() - t0:.1f} s, {v:.2f} Vpp, {rms:.3f} Vrms")
        assert got, f"no record at {d}: {rows}"
        near(f"RMS as Vpp at {d}", rms * 2 * 2**0.5, 2.0, rel=0.08)
    s.w(":ACQ:MDEP AUTO")
    return "; ".join(rows)


@test("a1", need=(1,))
def rates_for_the_record(s, o):
    """Update rate and web speed, figures for the docs."""
    c = s.loop[0]
    s.gen("SQU", 1e6, 2.0)
    s.w(":TIM:SCAL 0.00000005")
    s.settle(extra=0.5)
    a, t0 = s.stat()["frames"], time.time()
    time.sleep(5)
    w = (s.stat()["frames"] - a) / (time.time() - t0)
    t0 = time.time()
    n = 0
    while time.time() - t0 < 5:
        s.http("/screen.png")
        n += 1
    t0 = time.time()
    s.w(":ACQ:MDEP 1000000")
    s.settle()
    b = len(s.http("/wave.csv")[1])
    raise Info(f"{w:.0f} records/s at 50 ns/div, web {n / 5:.1f} pictures/s, /wave.csv {b / 1e6:.1f} MB in {time.time() - t0:.1f} s")


@test("a1", need=(1,))
def exports_match_the_screen(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e3, 2.0)
    s.settle(extra=0.3)
    st, b = s.http("/wave.csv")
    assert st == 200
    v = [float(l.split(",")[1]) for l in b.decode().splitlines()[1:] if l.count(",") and l.split(",")[1]]
    near("CSV Vpp", max(v) - min(v), 2.0, rel=0.06)
    st, b = s.http("/wave.bin")
    assert st == 200 and len(b) > 1000, (st, len(b))


@test("a1", need=(1,))
def protocol_trigger_and_more(s, o):
    """Duration, and the bus trigger on a UART byte from the generator."""
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.w(":TIM:SCAL 0.00005")
    s.gen("SQU", 1e4, 2.0, duty=50)
    pat = " ".join("H" if i == c - 1 else "X" for i in range(4))
    a = s.fires(trig_src=c - 1, trig_type="Duration", pat=pat, pat_levels="0 0 0 0", trig_when="Gt", trig_t1=40e-6)
    b = s.fires(trig_t1=60e-6)
    s.set(trig_type="Edge", trig_level=0)
    s.w(":TRIG:SWE AUTO")
    assert a > 0 and b == 0, f"duration: fires {a} and {b}"


@test("a1", need=(1,))
def time_axis_at_every_channel_count(s, o):
    """The generator's frequency, read from the record, from fast to roll: a wrong rate shows here at once."""
    c = s.loop[0]
    rows = {}
    for chans in ((c,), tuple(CH[:2]), tuple(CH)):
        for x in CH:
            s.w(f":CHAN{x}:DISP {1 if x in chans or x == c else 0}")
        for tb, f in ((1e-5, 5e4), (1e-3, 1e3), (5e-3, 1e3), (0.02, 250), (0.05, 100), (0.1, 50), (0.2, 25), (0.5, 10)):
            s.gen("SIN", f, 2.0)
            s.w(f":TIM:SCAL {tb}")
            time.sleep(tb * 13 + 2)
            rows[f"{len(chans)} ch {tb}"] = (f, s.meas("FREQ", c, 3))
    s.w(":SOUR:OUTP OFF")
    bad = {k: v for k, v in rows.items() if not abs(v[1] / v[0] - 1) < 0.02}
    assert not bad, f"{len(bad)} of {len(rows)}: {bad}"
    return f"{len(rows)} sets"


@test("a1", need=(1,))
def auto_fits_slow_and_fast_signals(s, o):
    c = s.loop[0]
    rows = {}
    for wave, f, vpp in (("SIN", 50, 2.0), ("SQU", 1e4, 3.0), ("SIN", 5e6, 0.2)):
        s.base()
        s.gen(wave, f, vpp)
        s.settle(extra=0.5)
        auto(s)
        rows[f] = (s.q(":TIM:SCAL?"), s.q(f":CHAN{c}:SCAL?"))
        near(f"frequency of {f} Hz", s.meas("FREQ", c, 3), f, rel=0.02)
        # VPP counts the overshoot of a square, VAMP needs a flat top
        near(f"amplitude of {f} Hz", s.meas("VAMP" if wave == "SQU" else "VPP", c, 3), vpp, rel=0.3)
        assert s.q(f":CHAN{c}:DISP?") == "1" and s.q(":TRIG:EDGE:SOUR?") == f"CHAN{c}", rows
    s.w(":SOUR:OUTP OFF")
    return str(rows)


@test("a1", need=(1,))
def auto_keeps_the_set_up_without_a_signal(s, o):
    if s.sim:
        raise Skip("the simulation always has signals")
    free = next(c for c in CH if c not in s.loop and not (o.rf and c == o.rf_ch) and c not in o.busy)
    for c in CH:
        s.w(f":CHAN{c}:DISP {1 if c == free else 0}")
    s.w(f":CHAN{free}:SCAL 0.5")
    s.w(":TIM:SCAL 0.0005")
    s.settle()
    auto(s)
    near("scale", s.num(f":CHAN{free}:SCAL?"), 0.5, rel=0.01)
    near("time base", s.num(":TIM:SCAL?"), 5e-4, rel=0.01)
    s.w(f":CHAN{free}:DISP 0")


@test("a1", need=(1,))
def roll_and_scan_show_the_signal(s, o):
    c = s.loop[0]
    s.w(f":CHAN{c}:SCAL 1")
    s.gen("SIN", 2.0, 2.0)
    s.w(":TIM:SCAL 0.2")
    s.settle(extra=5)
    s.shot("roll-1ch", o.out)
    a = s.meas("VPP", c, 1)
    s.set(auto_roll="false")
    time.sleep(6)
    s.shot("scan-1ch", o.out)
    b = s.meas("VPP", c, 1)
    s.set(auto_roll="true")
    s.w(":TIM:SCAL 0.0002")
    near("roll", a, 2.0, rel=0.1)
    near("scan", b, 2.0, rel=0.1)


@test("a1", need=(1,))
def zoom_and_views(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e4, 2.0)
    s.w(":TIM:SCAL 0.0002")
    s.set(zoom="true")
    s.settle(extra=0.4)
    s.shot("zoom", o.out)
    s.set(zoom="false", dots="false")
    s.settle(extra=0.3)
    s.shot("vectors", o.out)
    s.set(dots="true")
    t = s.q(":IDEN?")
    clean(s)
    return t[:120]


@test("a1", need=(1,))
def bode_of_the_cable(s, o):
    c = s.loop[0]
    f = s.q(":SYST:GET? bode").split()
    f[:3] = ["1000", "1000000", "8"]
    f[4] = f[5] = str(c)  # one channel against the generator
    s.set(bode=" ".join(f))
    s.w(":BODE:RUN ON")
    end = time.time() + 60
    t = ""
    while time.time() < end:
        t = s.q(":BODE:DATA?")
        if t.count(";") >= 7:
            break
        time.sleep(1)
    s.w(":BODE:RUN OFF")
    pts = [p.split(",") for p in t.split(";") if p]
    assert len(pts) >= 6, t[:200]
    g = [float(p[1]) for p in pts]
    assert max(abs(x) for x in g) < 1.5, g


@test("a1", need=(1,))
def analysers_show_the_signal(s, o):
    c = s.loop[0]
    s.gen("SIN", 1e5, 2.0)
    s.w(":TIM:SCAL 0.00001")
    for v in ("SPEC", "EYE", "SCOP"):
        s.w(f":VIEW {v}")
        time.sleep(1)
        s.shot(f"view-{v}", o.out)
    clean(s)
    assert s.q(":IDEN?") is not None


# Worst first: the analysers work on the whole record, so a deep record is what shows a slow one. The floors are about
# half of what the unit measured on 2026-11-07, low enough for a busy Mac, high enough to catch a slowed-down analyser.
VIEW_FPS_FLOOR = 8.0
VIEW_LATENCY_MAX = 1.0


def ui_cpu_ticks(s):
    """User + system clock ticks of oos-ui so far."""
    t = s.shell("for p in $(pidof oos-ui oos-ui.new); do cut -d' ' -f14,15 /proc/$p/stat; done")
    return sum(int(a) + int(b) for a, b in re.findall(r"^\s*(\d+) (\d+)\s*$", t, re.M))


@test("a1", need=(1,))
def analysers_keep_the_frame_rate(s, o):
    """Frame rate, records, SCPI answer time and CPU of every view on a square wave, normal and deep record."""
    if s.sim:
        raise Skip("the rates are the unit's")
    s.gen("SQU", 1e6, 2.0)
    s.w(":TIM:SCAL 0.000002")
    out, bad = [], []
    try:
        for depth in ("AUTO", "1000000"):
            s.w(f":ACQ:MDEP {depth}")
            for v in ("SCOP", "SPEC", "EYE", "POW"):
                s.w(f":VIEW {v}")
                time.sleep(2.5)
                a, c0, t0 = s.stat(), ui_cpu_ticks(s), time.time()
                worst, fps = 0.0, []
                for _ in range(8):
                    q = time.time()
                    st = s.stat()
                    worst = max(worst, time.time() - q)
                    fps.append(st["fps"])
                    time.sleep(0.5)
                dt, b, c1 = time.time() - t0, s.stat(), ui_cpu_ticks(s)
                f = sorted(fps)[len(fps) // 2]
                cpu = (c1 - c0) / dt  # 100 ticks per second = one core
                rec = (b["frames"] - a["frames"]) / dt
                out.append(f"{v}/{depth}: {f:.1f} fps, {rec:.1f} rec/s, {worst * 1e3:.0f} ms, CPU {cpu:.0f} %")
                if f < VIEW_FPS_FLOOR or worst > VIEW_LATENCY_MAX:
                    bad.append(out[-1])
    finally:
        s.w(":VIEW SCOP")
        s.w(":ACQ:MDEP AUTO")
        clean(s)
    assert not bad, bad
    return "; ".join(out)


@test("a1", need=(1,))
def generator_has_no_dc_in_any_gear(s, o):
    """The middle of a sine on both sides of the generator's range switch at 2 Vpp, and at 6 Vpp. The channel's own zero
    error is the same in every reading and drops out of the differences; GND coupling is no reference, the FPGA makes
    it exactly zero."""
    c = s.loop[0]
    s.w(f":CHAN{c}:DISP 1")
    s.w(f":TRIG:EDGE:SOUR CHAN{c}")
    s.w(f":CHAN{c}:SCAL 1")
    s.w(f":CHAN{c}:BWL 20M")
    s.w(":TIM:SCAL 0.0002")

    def mid(a):
        s.gen("SIN", 1e3, a)
        s.settle(extra=1.2)
        v = sorted(s.wave(c)[2])
        return (v[len(v) // 200] + v[-len(v) // 200]) / 2

    rows = {a: round(mid(a), 3) for a in (1.0, 2.0, 2.5, 4.0, 6.0)}
    s.w(":SOUR:OUTP OFF")
    s.w(f":CHAN{c}:BWL AUTO")
    step = round(rows[2.5] - rows[2.0], 3)
    assert abs(step) < 0.04 and abs(rows[6.0] - rows[1.0]) < 0.04, f"jump at 2 Vpp {step} V; middle by Vpp {rows}"
    return f"jump at 2 Vpp {step} V; middle by Vpp {rows}"


@test("a1", need=(1,))
def deep_memory_stays_responsive(s, o):
    """100 kHz in a 50 M record is a hundred thousand edges, for each frame's measurements; Auto from there is quick too."""
    s.gen("SIN", 1e5, 1.0)
    s.w(":TIM:SCAL 5e-6")
    s.w(":ACQ:MDEP 50000000")
    time.sleep(6)
    worst = 0.0
    for tb in (5e-6, 2e-4, 5e-3, 5e-2):
        s.w(f":TIM:SCAL {tb}")
        time.sleep(5)
        for _ in range(4):
            t = time.time()
            s.stat()
            worst = max(worst, time.time() - t)
    s.w(":TIM:SCAL 5e-6")
    time.sleep(3)
    t = time.time()
    s.w(":AUT")
    while time.time() - t < 15 and s.q(":AUT?").strip() not in ("0", "OFF"):
        time.sleep(0.3)
    took = time.time() - t
    depth = s.num(":ACQ:MDEP?")
    s.w(":ACQ:MDEP AUTO")
    clean(s)
    assert worst < 1.0 and took < 10 and depth < 1e7, f"answer {worst:.2f} s, Auto {took:.1f} s, depth {depth:g}"
    return f"answer {worst * 1e3:.0f} ms, Auto {took:.1f} s"


@test("a1", need=(1,))
def either_slope_fires(s, o):
    """Either on the trigger types that have a slope: it fires, on a square wave as often as a single slope or more."""
    c = s.loop[0]
    s.gen("SQU", 1e3, 2.0)
    s.w(":TIM:SCAL 0.0002")
    types = {
        "Edge": dict(trig_type="Edge", trig_level=0),
        "Slope": dict(trig_type="Slope", trig_when="Lt", trig_level=0.5, trig_level2=-0.5, trig_t1=1e-4),
        "NthEdge": dict(trig_type="NthEdge", trig_level=0, nth_n=1, nth_idle=1e-4),
    }
    if len(s.loop) > 1:
        # rising on one to the falling edge on the other: half a period
        s.w(f":CHAN{s.loop[1]}:DISP 1")
        types["Delay"] = dict(trig_src2=s.loop[1] - 1, trig_slope2="Fall", trig_type="Delay", trig_when="Lt", trig_t1=1e-3, trig_level=0, trig_level2=0)
    out, bad = [], []
    for name, kv in types.items():
        rise = s.fires(trig_src=c - 1, slope="Rise", **kv)
        either = s.fires(slope="Either")
        out.append(f"{name} rise {rise}, either {either}")
        if rise <= 0 or either <= 0:
            bad.append(out[-1])
    s.set(trig_type="Edge", slope="Rise", trig_level=0)
    s.w(":TRIG:SWE AUTO")
    clean(s)
    assert not bad, bad
    return "; ".join(out)


@test("a1", need=(1,))
def high_res_lowers_the_noise(s, o):
    """Point to point noise of a slow sine with the record longer than the screen: Normal against High res."""
    s.gen("SIN", 100.0, 1.0)
    s.w(":TIM:SCAL 0.0002")
    s.w(":ACQ:MDEP 100000")

    def noise():
        s.settle(extra=0.3)
        v = s.wave(s.loop[0])[2]
        return statistics.median(abs(b - a) for a, b in zip(v, v[1:]))

    s.set(acq="Normal")
    normal = noise()
    s.set(acq="HiRes")
    hires = noise()
    s.set(acq="Normal")
    s.w(":ACQ:MDEP AUTO")
    clean(s)
    assert hires < 0.6 * normal, f"normal {normal * 1e3:.2f} mV, high res {hires * 1e3:.2f} mV"
    return f"normal {normal * 1e3:.2f} mV, high res {hires * 1e3:.2f} mV"


@test("a1", need=(1,))
def formula_runs(s, o):
    s.gen("SIN", 1e3, 2.0)
    s.set(math="Expr", math_expr1=f"2*CH{s.loop[0]}+0.5")
    s.settle(extra=0.3)
    s.shot("math-formula", o.out)
    clean(s)
    s.set(math="Off")


@test("a1", need=(1,))
def logger_writes(s, o):
    if s.sim:
        raise Skip("the file is on the unit")
    s.gen("SIN", 1e3, 2.0)
    s.w(":SYST:LOG ON")
    time.sleep(6)
    s.w(":SYST:LOG OFF")
    t = s.shell("ls -t /tmp/oos/log-*.csv /oos/log-*.csv 2>/dev/null | head -1 | xargs wc -l")
    assert re.search(r"\b[4-9]\b", t), t


# ---------------------------------------------------------------------------------------------- a2: two channels


@test("a2", need=(2,))
def two_channels_agree(s, o):
    a, b = s.loop[:2]
    for c in (a, b):
        s.w(f":CHAN{c}:DISP 1")
        s.w(f":CHAN{c}:SCAL 1")
    s.gen("SIN", 1e5, 2.0)
    s.w(":TIM:SCAL 0.000002")
    s.settle(extra=0.3)
    near("amplitude", s.meas("VAMP", b), s.meas("VAMP", a), rel=0.03)
    _, dt, x = s.wave(a)
    _, _, y = s.wave(b)
    n = min(len(x), len(y))
    d = statistics.fmean(abs(x[i] - y[i]) for i in range(n))
    assert d < 0.15, f"mean difference {d:.3f} V"
    return f"mean difference {d * 1e3:.1f} mV"


@test("a2", need=(2,))
def deskew_moves_a_channel(s, o):
    """Direction and size: the first real measurement of it."""
    a, b = s.loop[:2]
    for c in (a, b):
        s.w(f":CHAN{c}:DISP 1")
    s.gen("SQU", 2e6, 2.0)
    s.w(":TIM:SCAL 0.0000001")

    def edge(c):
        t0, dt, v = s.wave(c)
        best = min((k for k in range(1, len(v)) if v[k - 1] < 0 <= v[k]), key=lambda k: abs(t0 + k * dt), default=None)
        return math.nan if best is None else t0 + best * dt

    s.settle(extra=0.3)
    d0 = edge(b) - edge(a)
    s.set(**{f"ch{b}__skew": 6e-9})
    s.settle(extra=0.4)
    d1 = edge(b) - edge(a)
    s.set(**{f"ch{b}__skew": 0})
    raise Info(f"edge CH{b} minus CH{a}: {d0 * 1e9:.2f} ns, with 6 ns deskew {d1 * 1e9:.2f} ns")


@test("a2", need=(2,))
def hidden_trigger_source(s, o):
    a, b = s.loop[:2]
    s.w(f":CHAN{a}:DISP 1")
    s.w(f":CHAN{b}:DISP 0")
    s.w(f":TRIG:EDGE:SOUR CHAN{b}")
    s.gen("SIN", 1e3, 2.0)
    assert s.fires() > 0, "no trigger from a channel that is off"
    s.w(":TRIG:SWE AUTO")
    s.w(f":TRIG:EDGE:SOUR CHAN{a}")
    assert s.num(f":CHAN{b}:DISP?") == 0


@test("a2", need=(2,))
def delay_trigger_between_channels(s, o):
    a, b = s.loop[:2]
    for c in (a, b):
        s.w(f":CHAN{c}:DISP 1")
    s.gen("SQU", 1e4, 2.0)
    # rising on one to the falling edge on the other: half a period
    n = s.fires(trig_src=a - 1, trig_src2=b - 1, trig_slope2="Fall", trig_type="Delay", trig_when="Lt", trig_t1=1e-4, trig_level=0, trig_level2=0)
    s.set(trig_type="Edge")
    s.w(":TRIG:SWE AUTO")
    assert n > 0


@test("a2", need=(2,))
def roll_with_all_loops(s, o):
    for c in s.loop:
        s.w(f":CHAN{c}:DISP 1")
    s.gen("SIN", 2.0, 2.0)
    s.w(":TIM:SCAL 0.2")
    time.sleep(6)
    s.shot("roll", o.out)
    v = [s.meas("VPP", c, 1) for c in s.loop]
    s.w(":TIM:SCAL 0.0002")
    assert all(x > 1.0 for x in v), v


# ---------------------------------------------------------------------------------------------- la: the risky one


@test("end")
def protected_areas_after(s, o):
    if not getattr(s, "finger", None):
        raise Skip("no fingerprint from the start of this run")
    assert fingerprint(s) == s.finger, "the card or the calibration changed during the run"


class Pico:
    """A Pico with the openSciLab firmware (protocol 8). A thread keeps its watchdog quiet: it drops the outputs after 1 s."""

    def __init__(self, port):
        import serial, threading

        self.p = serial.Serial(port, 115200, timeout=0.5)
        time.sleep(0.2)
        self.p.reset_input_buffer()
        self.stop = threading.Event()
        threading.Thread(target=self.beat, daemon=True).start()
        self.cmd(20)

    def beat(self):
        while not self.stop.wait(0.4):
            self.send(bytes([18]))

    def send(self, payload):
        o = bytearray(b"\x55\xaa")
        for b in payload:
            o += bytes([0xF0, b ^ 0xF0]) if b in (0xAA, 0x55, 0xF0) else bytes([b])
        self.p.write(bytes(o) + b"\xaa\x55")

    def cmd(self, c, data=b"", wait=2.0):
        self.send(bytes([c]) + data)
        end = time.time() + wait
        while time.time() < end:
            line = self.p.readline().decode("ascii", "replace").strip()
            if line:
                return line
        return ""

    def pwm(self, pin, hz, duty=32768):
        self.cmd(12, bytes([pin, 4]))
        return self.cmd(15, struct.pack("<BfH", pin, hz, duty))

    def pattern(self, data, rate, first, pins):
        self.cmd(21, struct.pack("<IB", 0, 0))
        self.cmd(21, struct.pack("<IB", 0, 0) + data)
        return self.cmd(22, struct.pack("<fBBIIBB", rate, first, pins, len(data), 0, 0, 0xFF))

    def uart(self, pin, baud, data):
        return self.cmd(26, struct.pack("<BI", pin, baud) + data)

    def spi(self, sck, mosi, cs, hz, mode, data):
        return self.cmd(27, struct.pack("<BBBBIB", sck, mosi, 0xFF, cs, hz, mode) + data)

    def i2c(self, sda, scl, addr, hz, data):
        return self.cmd(28, struct.pack("<BBBIH", sda, scl, addr, hz, 0) + data)

    def close(self):
        self.cmd(23)
        self.cmd(20)
        self.stop.set()
        self.p.close()


class Ht008:
    """HTOOL HT008: frames of 9 bytes, sine from 23.5 MHz, levels 0..3 = -4, -1, +2, +5 dBm."""

    def __init__(self, port):
        import serial

        self.p = serial.Serial(port, 115200, timeout=0.4)
        time.sleep(0.2)
        self.set(30e6)

    def set(self, hz, level=3):
        m, k = divmod(round(hz / 1e3), 1000)
        self.p.reset_input_buffer()
        self.p.write(b"\x55\x55" + m.to_bytes(2, "big") + k.to_bytes(2, "big") + bytes([level]) + b"\r\n")
        self.p.read(11)


def la_rec(s, wait=1.0):
    """(words, dt, t0) of the LA record."""
    time.sleep(wait)
    _, dt, t0 = s.q(":LA:PRE?", timeout=30).split(",")
    d = s.block(":LA:DATA?", timeout=90)
    return struct.unpack(f"<{len(d) // 2}H", d), float(dt), float(t0)


def la_edges(w, line):
    """Indices of the rising and of the falling edges of a line."""
    x = [(v >> line) & 1 for v in w]
    return [i for i in range(1, len(x)) if x[i] > x[i - 1]], [i for i in range(1, len(x)) if x[i] < x[i - 1]]


def la_freq(w, dt, line):
    r = la_edges(w, line)[0]
    return (len(r) - 1) / ((r[-1] - r[0]) * dt) if len(r) > 1 else 0.0


def with_pico(f):
    """A test of the la tier: LA on, the Pico open; both closed afterwards whatever happens."""

    def run(s, o):
        if s.sim or not o.pico:
            raise Skip("run with --pico <port>: D0, D1, D2 on GP2, GP3, GP4")
        p = Pico(o.pico)
        s.w(":LA:STAT ON")
        s.w(":LA:POD1:THR 1.65")
        s.settle(extra=2)
        try:
            return f(s, o, p)
        finally:
            p.close()
            s.w(":LA:POD1:THR 1.4")
            s.w(":LA:STAT OFF")
            s.settle()

    run.__name__ = f.__name__
    return run


@test("la")
@with_pico
def la_lines_in_order(s, o, p):
    s.w(":TIM:SCAL 1e-5")
    p.pattern(bytes(i % 8 for i in range(8)), 4e6, 2, 3)
    w, dt, _ = la_rec(s, 2)
    for line, f in ((0, 2e6), (1, 1e6), (2, 5e5)):
        near(f"D{line}", la_freq(w, dt, line), f, rel=0.002)
    assert not any(v & 0xfff8 for v in w), "a line without a signal moved"


@test("la")
@with_pico
def la_pulse_widths(s, o, p):
    s.w(":TIM:SCAL 1e-6")
    rows = []
    for n, total in ((1, 50), (5, 50), (25, 100)):
        p.pattern(bytes([1] * n + [0] * (total - n)), 50e6, 3, 1)
        w, dt, _ = la_rec(s, 1.5)
        r, fa = la_edges(w, 1)
        hi = [min(q for q in fa if q > a) - a for a in r if any(q > a for q in fa)]
        assert hi, f"no pulse of {n * 20} ns"
        near(f"{n * 20} ns", statistics.median(hi) * dt * 1e9, n * 20, ab=dt * 1e9 * 1.5)
        rows.append(round(statistics.median(hi) * dt * 1e9, 1))
        p.cmd(23)
    return f"{rows} ns"


@test("la")
@with_pico
def la_threshold_decides(s, o, p):
    """3.3 V logic: every line high below the low level... and never high above the high one."""
    s.w(":TIM:SCAL 1e-5")
    p.pwm(2, 1e6)
    got = {}
    for v in (1.65, 4.0, -1.0):
        s.w(f":LA:POD1:THR {v}")
        w, _, _ = la_rec(s, 1.5)
        got[v] = {x & 1 for x in w}
    s.w(":LA:POD1:THR 1.65")
    if got[1.65] == {0, 1} and got[4.0] == {0} and got[-1.0] == {1}:
        return "the threshold decides"
    # stock does the same on the same probe (docs/protocols/la.md): worth a look if this ever changes, no failure
    raise Info(f"the threshold has no effect, as under stock: levels of D0 per threshold {got}")


@test("la")
@with_pico
def la_trigger_edge_and_pulse(s, o, p):
    s.w(":TIM:SCAL 1e-4")
    p.pwm(3, 1000)
    a = s.fires(wait=2, trig_type="Edge", trig_la=1)
    w, dt, t0 = la_rec(s, 0.5)
    r, f = la_edges(w, 1)
    i0 = -t0 / dt
    off = min((abs(i - i0) * dt for i in r), default=1.0)
    quiet = s.fires(wait=2, trig_la=0)
    p.pwm(3, 1000, 16384)  # 250 us high
    b = s.fires(wait=2, trig_la=1, trig_type="Pulse", trig_when="Gt", trig_t1=200e-6)
    c = s.fires(wait=2, trig_t1=300e-6)
    s.set(trig_type="Edge", trig_la="none")
    s.w(":TRIG:SWE AUTO")
    assert a > 0 and quiet == 0 and b > 0 and c == 0, f"edge {a}, quiet line {quiet}, pulse {b}, {c}"
    assert off < 1e-6, f"the edge is {off} s from the trigger point"


@test("la")
@with_pico
def la_decoders(s, o, p):
    import threading

    def run(send, tb, want, **kv):
        s.w(f":TIM:SCAL {tb}")
        s.set(dec_on="true", dec_ascii="false", **kv)
        stop = threading.Event()

        def pump():
            while not stop.is_set():
                send()
                time.sleep(0.002)

        th = threading.Thread(target=pump, daemon=True)
        th.start()
        time.sleep(3)
        rows = []
        for _ in range(8):
            rows += [r.split(",") for r in s.q(":DEC:EVEN?").splitlines() if r.strip()]
            time.sleep(0.35)
        stop.set()
        th.join()
        s.set(dec_on="false", dec_ascii="true")
        good = sum(1 for r in rows if len(r) > 3 and r[2].upper() == want)
        assert good > 2 and good >= 0.3 * len(rows), f"{want}: {good} of {len(rows)}: {rows[:5]}"
        return good

    n = [run(lambda: p.uart(2, 115200, b"OOS"), 2e-4, "4F", dec_proto="Uart", dec_la="1 0 - - -", dec_baud=7, dec_bits=8)]
    n.append(run(lambda: p.spi(2, 3, 4, 1000000, 0, b"\xa5\x3c"), 1e-4, "A5", dec_proto="Spi", dec_la="1 1 0 2 -", dec_bits=8, dec_mode=0))
    n.append(run(lambda: p.i2c(2, 3, 0x50, 100000, b"\x12"), 1e-4, "50 W N", dec_proto="I2c", dec_la="1 0 1 - -"))
    s.set(dec_la="0 8 10 - -")
    return f"UART, SPI, I2C: {n} events"


@test("la")
@with_pico
def la_beside_analog_and_alone(s, o, p):
    s.w(":TIM:SCAL 1e-4")
    p.pattern(bytes(i % 8 for i in range(8)), 8e4, 2, 3)
    for shown in (1, 0):
        s.w(f":CHAN1:DISP {shown}")
        w, dt, _ = la_rec(s, 3)
        near(f"D0, CH1 {shown}", la_freq(w, dt, 0), 4e4, rel=0.002)
    s.w(":CHAN1:DISP 1")
    s.settle()


@test("la")
@with_pico
def la_time_axis_at_every_channel_count(s, o, p):
    """Normal acquisition and roll, for one, two and four channels. D0 and D2 at the same time base."""
    rows = {}
    for chans in ((1,), (1, 2), (3, 4), (1, 3), (1, 2, 3, 4)):
        for c in CH:
            s.w(f":CHAN{c}:DISP {1 if c in chans else 0}")
        for tb, f in ((1e-6, 1e5), (1e-4, 2e3), (5e-3, 100), (0.1, 50), (0.2, 50), (0.5, 50)):
            p.pwm(2, f)
            s.w(f":TIM:SCAL {tb}")
            w, dt, _ = la_rec(s, tb * 13 + 2.5)
            rows[f"{chans} {tb}"] = (f, round(la_freq(w, dt, 0), 1))
    bad = {k: v for k, v in rows.items() if not abs(v[1] / v[0] - 1) < 0.01}
    for c in CH:
        s.w(f":CHAN{c}:DISP {1 if c == 1 else 0}")
    assert not bad, f"{len(bad)} of {len(rows)}: {bad}"
    return f"{len(rows)} time base and channel sets"


def only_channels(s, chans):
    for c in CH:
        s.w(f":CHAN{c}:DISP {1 if c in chans else 0}")


def la_spacing(w, dt, line=0):
    """(longest / median distance of the rising edges, the record's length in s): a lost chunk is a long distance."""
    r = la_edges(w, line)[0]
    d = [b - a for a, b in zip(r, r[1:])]
    assert d, f"no edges on D{line}"
    return max(d) / statistics.median(d), len(w) * dt


@test("la")
@with_pico
def la_has_no_gaps_at_slow_time_bases(s, o, p):
    """Slow time bases below roll: every chunk of the LA stream arrives, with one, two and four channels."""
    rows, bad = {}, {}
    # D0 and D2 come from separate PWM units of the Pico: a piece the scope loses is missing on both. They are set once:
    # setting them again restarts their period, which on both lines looks just like a lost piece.
    p.pwm(2, 100)
    p.pwm(4, 100)
    try:
        for chans in ((1,), (1, 2), (3, 4), (1, 3), (1, 2, 3, 4)):
            only_channels(s, chans)
            for tb in (0.01, 0.02, 0.05, 0.1):
                s.w(f":TIM:SCAL {tb}")
                w, dt, _ = la_rec(s, tb * 13 + 2.5)
                (w0, span), (w2, _) = la_spacing(w, dt, 0), la_spacing(w, dt, 2)
                k = f"{chans} {tb}"
                rows[k] = (round(w0, 2), round(w2, 2), round(span / (10 * tb), 2), len(w))
                if min(w0, w2) > 1.05 or span < 0.95 * 10 * tb:
                    bad[k] = rows[k]
                elif max(w0, w2) > 1.05:
                    rows[k] += ("one line only: the source",)
    finally:
        only_channels(s, (1,))
    one = {k: v for k, v in rows.items() if len(v) > 4}
    assert not bad, f"{len(bad)} of {len(rows)} (longest distance D0, D2, part of the screen, words): {bad}"
    return f"{len(rows)} sets, worst distance {max(max(v[0], v[1]) for v in rows.values())}, on one line only: {one}"


@test("la")
@with_pico
def la_long_record(s, o, p):
    """Deep memory: the record holds what the depth promises, without gaps."""
    rows = {}
    p.pwm(2, 1000)
    try:
        for depth in (1000000, 10000000):
            s.w(f":ACQ:MDEP {depth}")
            s.w(":TIM:SCAL 0.001")
            w, dt, _ = la_rec(s, 4)
            worst, span = la_spacing(w, dt)
            rows[depth] = (len(w), round(worst, 2), round(span * 1e3, 1))
            assert worst < 1.05, f"depth {depth}: {rows[depth]}"
            assert span >= 0.0095, f"depth {depth}: {rows[depth]}"
    finally:
        s.w(":ACQ:MDEP AUTO")
    return f"(words, longest distance, ms): {rows}"


@test("la")
@with_pico
def la_in_scan_mode(s, o, p):
    s.set(auto_roll="false")
    rows = {}
    try:
        for tb, f in ((0.2, 50), (0.5, 50)):
            p.pwm(2, f)
            s.w(f":TIM:SCAL {tb}")
            w, dt, _ = la_rec(s, tb * 13 + 3)
            rows[tb] = round(la_freq(w, dt, 0), 2)
            near(f"D0 at {tb} s/div", rows[tb], f, rel=0.01)
    finally:
        s.set(auto_roll="true")
        s.w(":TIM:SCAL 0.0002")
    return f"{rows}"


@test("la")
@with_pico
def la_pattern_and_duration_trigger(s, o, p):
    """Pattern and duration over D lines: D0 has 250 us high at 1 kHz, D3 has no signal."""
    s.w(":TIM:SCAL 1e-4")
    p.pwm(2, 1000, 16384)
    x = "X" * 15
    a = s.fires(wait=2, trig_type="Pattern", pat="X X X X", pat_la="H" + x)
    b = s.fires(wait=2, pat_la="XXXH" + "X" * 12)
    c = s.fires(wait=2, trig_type="Duration", pat_la="H" + x, trig_when="Gt", trig_t1=200e-6)
    d = s.fires(wait=2, trig_t1=300e-6)
    s.set(trig_type="Edge", pat_la="X" * 16)
    s.w(":TRIG:SWE AUTO")
    assert a > 0 and b == 0 and c > 0 and d == 0, f"pattern {a}, quiet line {b}, duration {c}, too long {d}"
    return f"pattern {a}, duration {c}"


@test("la")
@with_pico
def la_csv_matches_the_data(s, o, p):
    """/la.csv and :LA:DATA? of one stopped record: the same states at the same times."""
    s.w(":TIM:SCAL 1e-5")
    p.pattern(bytes(i % 8 for i in range(8)), 4e6, 2, 3)
    time.sleep(2)
    s.w(":STOP")
    try:
        w, dt, t0 = la_rec(s, 1)
        code, body = s.http("/la.csv")
        assert code == 200, code
        rows = [r.split(",") for r in body.decode().splitlines()]
        lines = [int(h[1:]) for h in rows[0][1:]]
        assert 0 in lines and 2 in lines, rows[0]
        for r in rows[1:]:
            k = round((float(r[0]) - t0) / dt)
            for n, v in zip(lines, r[1:]):
                assert (w[k] >> n) & 1 == int(v), f"word {k}, D{n}: {r}"
    finally:
        s.w(":RUN")
    return f"{len(rows) - 1} rows, {len(w)} words"


@test("la")
@with_pico
def la_protocol_trigger(s, o, p):
    """Protocol trigger with the decoder on the LA: UART on D0, a byte that comes and one that does not."""
    import threading

    stop = threading.Event()

    def pump():
        while not stop.is_set():
            p.uart(2, 115200, b"OOS")
            time.sleep(0.002)

    s.w(":TIM:SCAL 2e-4")
    s.set(dec_on="true", dec_ascii="false", dec_proto="Uart", dec_la="1 0 - - -", dec_baud=7, dec_bits=8)
    th = threading.Thread(target=pump, daemon=True)
    th.start()
    try:
        s.w(":TRIG:SWE NORM")
        s.set(trig_type="Bus", trig_bus="Data")
        seen = {}
        for val in (0x4F, 0x7E):
            s.set(trig_bus_val=val)
            time.sleep(1.5)
            seen[val] = [s.stat()["status"] for _ in range(8)]
        # Single stops on a record with the event, and on no other: with a byte that never comes it keeps waiting
        singles = {}
        for val in (0x4F, 0x7E):
            s.set(trig_bus_val=val)
            s.w(":SING")
            time.sleep(2.5)
            singles[val] = s.stat()["run"]
            s.w(":STOP")
    finally:
        stop.set()
        th.join()
        s.set(trig_type="Edge", dec_on="false", dec_ascii="true", dec_la="0 8 10 - -")
        s.w(":TRIG:SWE AUTO")
    assert set(seen[0x4F]) == {"Triggered"} and set(seen[0x7E]) == {"Wait"}, seen
    assert singles == {0x4F: "Stop", 0x7E: "Run"}, singles
    return f"{seen[0x4F][0]} on a byte that comes, {seen[0x7E][0]} on one that does not, Single {singles}"


def with_rf(f):
    def run(s, o):
        if s.sim or not o.rf:
            raise Skip("run with --rf <port>")
        c = o.rf_ch
        g = Ht008(o.rf)
        for x in CH:
            s.w(f":CHAN{x}:DISP {1 if x == c else 0}")
        s.w(f":CHAN{c}:SCAL 0.2")
        s.w(f":CHAN{c}:BWL OFF")
        s.w(f":TRIG:EDGE:SOUR CHAN{c}")
        s.w(":ACQ:MDEP 10000")
        s.w(":TIM:SCAL 1e-7")
        s.settle(extra=1)
        try:
            return f(s, c, g)
        finally:
            s.w(":ACQ:MDEP AUTO")
            s.w(f":CHAN{c}:BWL AUTO")
            g.p.close()

    run.__name__ = f.__name__
    return run


@test("rf")
@with_rf
def rf_frequency_to_nyquist(s, c, g):
    rows = {}
    for f in (30e6, 100e6, 200e6, 300e6):
        g.set(f)
        s.w(":TIM:SCAL 2e-8")
        s.w(f":CHAN{c}:SCAL {0.2 if f < 250e6 else 0.05}")  # a third of a division is too little to count on
        s.settle(extra=0.4)
        rows[f / 1e6] = s.meas("FREQ", c, 3)
        near(f"{f / 1e6:g} MHz", rows[f / 1e6], f, rel=0.002)
    return str(rows)


@test("rf")
@with_rf
def rf_trigger_stays_on_the_edge(s, c, g):
    s.w(":TRIG:SWE NORM")
    rows = {}
    for f, tb in ((30e6, 1e-8), (100e6, 5e-9), (200e6, 2e-9)):
        g.set(f)
        s.w(f":TIM:SCAL {tb}")
        s.settle(extra=0.8)
        ts = []
        for _ in range(12):
            t0, dt, v = s.wave(c)
            z = [t0 + (i - v[i] / (v[i + 1] - v[i])) * dt for i in range(len(v) - 1) if v[i] < 0 <= v[i + 1]]
            if z:
                ts.append(min(z, key=abs))
        rows[f / 1e6] = round(statistics.pstdev(ts) * 1e12)
        assert rows[f / 1e6] < 100, f"{f / 1e6:g} MHz: {rows[f / 1e6]} ps"
    s.w(":TRIG:EDGE:LEV 1.5")
    time.sleep(0.5)
    a = s.stat()["frames"]
    time.sleep(1.5)
    n = s.stat()["frames"] - a
    s.w(":TRIG:EDGE:LEV 0")
    s.w(":TRIG:SWE AUTO")
    assert n == 0, f"{n} records with the level above the signal"
    return f"sd in ps: {rows}"


@test("rf")
@with_rf
def rf_limits_act_at_their_frequency(s, c, g):
    """Against Full at the same frequency, so the response of the source drops out."""
    rows = {}
    for f, bw in ((100e6, "100M"), (200e6, "200M"), (50e6, "20M")):
        g.set(f)
        s.w(f":CHAN{c}:BWL OFF")
        s.settle(extra=0.5)
        a = s.sine(c, f)
        s.w(f":CHAN{c}:BWL {bw}")
        s.settle(extra=0.5)
        b = s.sine(c, f)
        rows[bw] = round(20 * math.log10(b / a), 1)
    s.w(f":CHAN{c}:BWL OFF")
    assert rows["20M"] < -3 and rows["100M"] < -1 and rows["200M"] < -1, str(rows)
    return f"dB against Full: {rows}"


def auto(s):
    s.w(":AUT")
    time.sleep(0.3)
    end = time.time() + 30
    while s.q(":AUT?") == "1":
        assert time.time() < end, "auto set-up does not end"
        time.sleep(0.1)
    time.sleep(1)


@test("rf")
@with_rf
def rf_auto_stays_on_the_channels_that_are_on(s, c, g):
    """Only the source's channel on, the trigger left elsewhere: Auto takes that channel and switches nothing on."""
    s.w(":ACQ:MDEP AUTO")
    s.w(f":TRIG:EDGE:SOUR CHAN{1 if c != 1 else 2}")
    g.set(30e6)
    auto(s)
    on = [x for x in CH if s.q(f":CHAN{x}:DISP?") == "1"]
    assert on == [c], f"channels on: {on}"
    assert s.q(":TRIG:EDGE:SOUR?") == f"CHAN{c}", s.q(":TRIG:EDGE:SOUR?")
    near("time base", s.num(":TIM:SCAL?"), 1e-8, rel=2.0)
    near("frequency", s.meas("FREQ", c, 3), 30e6, rel=0.01)


@test("rf")
@with_rf
def rf_hidden_trigger_source(s, c, g):
    other = 1 if c != 1 else 2
    s.w(":ACQ:MDEP AUTO")
    s.w(f":CHAN{other}:DISP 1")
    s.w(f":CHAN{c}:DISP 0")
    s.w(":TIM:SCAL 2e-4")
    a = s.fires(wait=1.5)
    s.w(":TRIG:EDGE:LEV 2")
    b = s.fires(wait=1.5)
    s.w(":TRIG:EDGE:LEV 0")
    s.w(":TRIG:SWE AUTO")
    assert a > 0 and b == 0, f"fires {a}, level above the signal {b}"


# ---------------------------------------------------------------------------------------------- runner


def soak(s, o, minutes, log):
    """Hours of the same: does anything grow, stop or heat up?"""
    s.base()
    if s.loop:
        s.gen("SIN", 1e3, 2.0)
    rows = []
    end = time.time() + minutes * 60
    while time.time() < end:
        st = s.stat()
        r = {"t": round(time.time()), "frames": st["frames"], "fps": st["fps"], "status": st["status"]}
        try:
            r["info"] = s.shell("grep VmRSS /proc/$(pidof oos-ui oos-ui.new | cut -d' ' -f1)/status; cat /sys/class/thermal/thermal_zone0/temp; free | sed -n 2p")
        except Skip:
            pass
        rows.append(r)
        log(f"soak {len(rows)}: {r}")
        with open(os.path.join(o.out, "soak.json"), "w") as f:
            json.dump(rows, f)
        time.sleep(min(60, minutes * 6))
    assert rows[-1]["frames"] > rows[0]["frames"], "frames stopped"
    return f"{len(rows)} samples, frames {rows[0]['frames']} to {rows[-1]['frames']}"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="192.168.177.64")
    ap.add_argument("--scpi-port", type=int, default=5555)
    ap.add_argument("--web-port", type=int, default=8080)
    ap.add_argument("--tier", help="a0, a1, a2, la (default: all the wiring allows)")
    ap.add_argument("--only", help="tests whose name contains this")
    ap.add_argument("--pico", help="serial port of the Pico on D0-D2")
    ap.add_argument("--rf", help="serial port of the HT008")
    ap.add_argument("--rf-ch", type=int, default=2, help="channel the HT008 is on")
    ap.add_argument("--busy", type=int, nargs="*", default=[], help="other channels with a cable: no open inputs")
    ap.add_argument("--plan", help="output of tools/plan-matrix.sh on this computer")
    ap.add_argument("--baseline", help="fingerprint.json of an earlier run")
    ap.add_argument("--soak-min", type=float, default=0)
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--sim", action="store_true", help="start oos-ui --serve from ui/target/debug, generator on CH1 and CH2")
    ap.add_argument("--out")
    o = ap.parse_args()
    o.out = o.out or os.path.join("device-test", time.strftime("%Y%m%d-%H%M%S"))
    os.makedirs(o.out, exist_ok=True)
    proc = None
    if o.sim:
        here = os.path.dirname(os.path.abspath(__file__))
        proc = subprocess.Popen([os.path.join(here, "../ui/target/debug/oos-ui"), "--serve", "--scpi-port", "15555", "--web-port", "18080"], env={**os.environ, "OOS_DEMO_LOOP": "3"}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        o.host, o.scpi_port, o.web_port = "127.0.0.1", 15555, 18080
        time.sleep(1.5)

    def log(t):
        line = f"{time.strftime('%H:%M:%S')} {t}"
        print(line, flush=True)
        with open(os.path.join(o.out, "run.log"), "a") as f:
            f.write(line + "\n")

    results = []
    gone = None
    s = None

    def stop(*_):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop)
    try:
        s = Scope(o.host, o.scpi_port, o.web_port, sim=o.sim)
        log(f"scope: {s.q('*IDN?')}")
        for tier, name, f, need in TESTS:
            always = name in ("generator_reaches", "protected_areas_before", "protected_areas_after")
            if o.tier and tier != o.tier and not always:
                continue
            if o.only and o.only not in name and not always:
                continue
            t0 = time.time()
            try:
                if need and len(s.loop) < need[0]:
                    raise Skip(f"generator on {need[0]} channel(s) needed, found {len(s.loop)}")
                s.base()
                time.sleep(0.2)
                detail = f(s, o)
                res = "PASS"
                left = s.errors()
                if left:
                    res, detail = "FAIL", f"SCPI errors: {left[:3]}"
            except Skip as e:
                res, detail = "SKIP", str(e)
            except Info as e:
                res, detail = "INFO", str(e)
            except Gone as e:
                res, detail = "GONE", str(e)
                gone = name
            except AssertionError as e:
                res, detail = "FAIL", str(e) or traceback.format_exc(limit=2)
            except Exception:
                res, detail = "ERROR", traceback.format_exc(limit=3)
            results.append({"tier": tier, "test": name, "result": res, "s": round(time.time() - t0, 1), "detail": str(detail or "")})
            log(f"{res:5} {tier} {name} ({time.time() - t0:.0f} s) {str(detail or '')[:160]}")
            if res in ("FAIL", "ERROR"):
                try:
                    s.shot(f"fail-{name}", o.out)
                except Exception:
                    pass
            with open(os.path.join(o.out, "results.json"), "w") as fh:
                json.dump(results, fh, indent=1)
            if gone:
                break
        if o.soak_min and not gone:
            try:
                d = soak(s, o, o.soak_min, log)
                results.append({"tier": "soak", "test": "soak", "result": "PASS", "s": o.soak_min * 60, "detail": d})
            except (AssertionError, Gone) as e:
                results.append({"tier": "soak", "test": "soak", "result": "FAIL", "s": 0, "detail": str(e)})
        if not gone:
            s.base()
    except Gone as e:
        gone = gone or "start"
        log(f"GONE: {e}")
    except KeyboardInterrupt:
        # never leave the unit in a test's state: an analyser view or a running generator looks like a fault
        log("stopped: back to the waveform view")
        if s:
            s.base()
    finally:
        if proc:
            proc.terminate()
    count = {k: sum(1 for r in results if r["result"] == k) for k in ("PASS", "FAIL", "ERROR", "INFO", "SKIP", "GONE")}
    with open(os.path.join(o.out, "report.md"), "w") as f:
        f.write(f"# OOS device test {os.path.basename(o.out)}\n\n{count}\n\n| Result | Tier | Test | s | Detail |\n|---|---|---|---|---|\n")
        for r in results:
            f.write(f"| {r['result']} | {r['tier']} | {r['test']} | {r['s']} | {r['detail'][:300].replace('|', '/').replace(chr(10), ' ')} |\n")
    log(f"done: {count}, report in {o.out}")
    sys.exit(1 if count["FAIL"] or count["ERROR"] or count["GONE"] or gone else 0)


if __name__ == "__main__":
    import urllib.parse

    main()
