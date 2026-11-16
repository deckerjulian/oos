#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""Check on an MHO900 under its stock system what OOS assumes about its acquisition board.

  mho-check.py <scope-ip> [--adb-port 55555] [--scpi-port 5555] [--no-scpi] [--yes]

Every assumption of docs/protocols/mho-acquisition.md that can be seen without writing to the hardware is checked, and
the result is written to mho-check-<model>-<date>.txt in the current directory, to attach to the GitHub issue.

What it does on the scope:
- over adb (network adb on, as for tools/platform-probe.sh; adbd is restarted as root as that tool does) it reads
  files under /sys, /proc and /rigol, lists devices and modules, and reads FPGA registers with the stock tool
  /rigol/tools/pcie2dev. That tool reads one register when it is given exactly one address and writes when it is
  given two; this script only ever gives it one address, from the fixed list below, and only addresses the stock
  system itself uses (a read of an address the FPGA does not decode could stop the processor). Nothing is written to
  a file, a partition or a register.
- over SCPI (port 5555) it changes the scope's settings the way a user would at the front panel (channels on and
  off, time base, memory depth, waveform format, the mask test on and off, Run/Stop) and reads back registers after
  each change. The settings it changes are read first, set back at the end and read again (R16 says whether every one
  is as before). --no-scpi leaves the settings alone and skips those checks.

What it does not collect: the serial number (left out of *IDN?), the FPGA DNA, MAC addresses, calibration or licence
files, the content of the clock tables (only their line count and format).
"""
import argparse
import datetime
import re
import socket
import subprocess
import sys
import time

VERSION = "2"
PCIE2DEV = "/rigol/tools/pcie2dev"

# Registers read once: (address, name). Only addresses the stock MHO900 system reads or writes itself.
STATIC_REGS = [
    (0x0004, "FPGA version"),
    (0x401C, "hardware version"),
    (0x1000, "SPU control"),
    (0x2028, "TPU trigger A"),
    (0x3000, "ADC SPI bridge, last word"),
    (0x3004, "ADC SPI bridge, read data"),
    (0x5000, "Bode block"),
    (0x2438, "WAU mask control"),
    (0x6110, "mask control (moved from 0x1190)"),
    (0x10100, "peripheral PWM"),
    (0x20004, "JESD204 receiver"),
    (0x20400, "JESD204 receiver, second bank"),
    (0x30000, "multi-channel sync"),
    (0x11E8, "TX state"),
    (0x1208, "recorded range"),
]

# Registers read after each change of the settings.
SNAP_REGS = [
    0x4004, 0x4008, 0x400C, 0x4010, 0x4014, 0x4018, 0x4030, 0x4034, 0x4050, 0x4054,
    0x10D0, 0x10D4, 0x10D8, 0x10E0, 0x10E4, 0x1100, 0x1104, 0x1108, 0x110C, 0x11F0,
    0x3000, 0x40B0, 0x40B4, 0x4100, 0x6110,
]

# The SPI port the bitstream goes over is not in the list: the stock device tree gives spi1 and spi5 a spidev each, with
# only one of them enabled; S6 records which /dev/spidev* there is.
DEVICES = ["/dev/xdma0_bypass", "/dev/xdma0_c2h_0", "/dev/spi_3wires", "/dev/spi2k7_ctl",
           "/dev/spi_simulate", "/dev/afe_rms", "/dev/hdcode_gpio"]
MODULES = ["xdma", "pcie_rockchip", "spi2pll_gpio", "fpga_gpio", "spi2afe_gpio", "afe_rms_gpio", "dac_gpio",
           "hdcode_gpio"]
# (gpio number, line, expected direction, expected value or None)
POWER_LINES = [(1, "GPIO0_A1 ADC supply", "out", "1"), (2, "GPIO0_A2 front-end supply", "out", "1"),
               (32, "GPIO1_A0 FPGA supply", "out", "1"), (120, "GPIO3_D0 LA probe detect", "in", None)]
# The FPGA image per hardware code that hal/src/mho/acq.rs assumes.
IMAGES = {1: "sparrow3_K160T.bit", **{c: "sparrow3_RT688.bit" for c in range(2, 8)},
          **{c: "sparrow3_K160T_LR.bit" for c in range(8, 12)}}

READ_RE = re.compile(r"Read\((0x[0-9a-fA-F]+|\d+)\)\s*=\s*#0x([0-9a-fA-F]+)#")
LOG_RE = re.compile(r"^\[\s*(\d+\.\d+)\]\s*(.*)$")


# ---- pure parts (tools/tests/test_mho_check.py) ----

def remote_read_command(addrs):
    """The one shell line that reads `addrs` on the scope: pcie2dev with exactly one address each."""
    for a in addrs:
        if not isinstance(a, int) or a < 0 or a >= 1 << 24 or a % 4:
            raise ValueError(f"not a register address: {a!r}")
    words = " ".join(f"0x{a:x}" for a in addrs)
    return f'for a in {words}; do {PCIE2DEV} "$a"; done'


def parse_reads(text):
    """{address: value} from pcie2dev's output."""
    return {int(a, 0): int(v, 16) for a, v in READ_RE.findall(text)}


def parse_versions(text):
    """{hardware code: image} from /rigol/resource/conf/version.txt (lines `0x1@name.bit`)."""
    out = {}
    for line in text.splitlines():
        m = re.match(r"\s*(0x[0-9a-fA-F]+|\d+)\s*@\s*(\S+)", line)
        if m:
            out[int(m.group(1), 0)] = m.group(2)
    return out


def bar_sizes(resource):
    """Sizes of the regions in a PCI device's `resource` file (start end flags per line)."""
    sizes = []
    for line in resource.splitlines():
        f = line.split()
        if len(f) == 3:
            start, end = int(f[0], 16), int(f[1], 16)
            if end > start:
                sizes.append(end - start + 1)
    return sizes


def adc_bridge_word(w):
    """(ok, text) for the last word in 0x3000: strobe (bit 26) cleared, exactly one ADC select (bit 24 or 25)."""
    if w == 0:
        return None, "0 (no transfer since power-on, or a register that reads 0)"
    sel = (w >> 24) & 3
    strobe = (w >> 26) & 1
    ok = strobe == 0 and sel in (1, 2) and (w >> 27) == 0
    kind = "read" if w & (1 << 23) else "write"
    return ok, (f"0x{w:08x}: strobe {strobe}, ADC {'0' if sel == 1 else '1' if sel == 2 else '?'} (select {sel}), "
                f"{kind}, address 0x{(w >> 16) & 0x7f:02x}, data 0x{w & 0xffff:04x}")


def fpga_done(log):
    """(status, text) for the FPGA's configuration from the kernel log. The driver logs DONE when it requests the line,
    before the image is loaded, so a DONE of 0 there says nothing; the FPGA's PCIe endpoint enumerating afterwards
    says the image is in."""
    done, pcie = [], None
    for line in log.splitlines():
        m = LOG_RE.match(line.strip())
        t, msg = (float(m.group(1)), m.group(2)) if m else (None, line)
        d = re.search(r"PIN_DONE\s+voltage:\s*(\d)", msg)
        if d:
            done.append((t, d.group(1)))
        if pcie is None and re.search(r"\[10ee:7024\]", msg):
            pcie = t
    if pcie is not None:
        logged = f"; DONE logged {done[-1][1]} at {done[-1][0]} s, when the line was requested" if done else ""
        return "PASS", f"PCIe endpoint 10ee:7024 enumerated at {pcie} s (the image is loaded){logged}"
    if any(v == "1" for _, v in done):
        return "PASS", "DONE 1"
    if done:
        return "FAIL", f"DONE {done[-1][1]} and no PCIe endpoint in the log"
    return "INFO", "not in the kernel log (needs adb root, or the log has rotated)"


def depth_word(answer):
    """The memory depth as :ACQuire:MDEPth takes it (1k, 10k, 1M, AUTO ...) from what its query answers
    (1.0000E+04): the scope refuses the number form with -222."""
    a = answer.strip()
    if a.upper().startswith("AUTO"):
        return "AUTO"
    try:
        n = round(float(a))
    except ValueError:
        return a
    for unit, f in (("G", 10**9), ("M", 10**6), ("k", 10**3)):
        if n >= f and n % f == 0:
            return f"{n // f}{unit}"
    return str(n)


def channel_mask(v4008):
    return v4008 & 0xF


def preamble_check(pre, scale):
    """(ok, text): WORD preamble with yincrement = scale / 7500 and the centre at 32768."""
    f = [x.strip() for x in pre.split(",")]
    if len(f) < 10:
        return False, f"preamble has {len(f)} fields: {pre!r}"
    try:
        fmt, yinc, yorg, yref = int(float(f[0])), float(f[7]), float(f[8]), float(f[9])
    except ValueError:
        return False, f"preamble not numeric: {pre!r}"
    if fmt != 1:
        return False, f"format {fmt}, not WORD (1): {pre!r}"
    codes = scale / yinc if yinc else float("inf")
    centre = yorg + yref
    ok = abs(codes - 7500.0) < 7500.0 * 1e-3 and abs(centre - 32768.0) <= 7500.0 * 10
    return ok, (f"{codes:.1f} codes per division (expected 7500), yorigin {yorg:g} + yreference {yref:g} = {centre:g} "
                f"(expected 32768 plus the offset in codes)")


def idn_without_serial(idn):
    f = [x.strip() for x in idn.split(",")]
    if len(f) >= 4:
        f[2] = "<serial>"
    return ",".join(f)


# ---- the scope ----

class Adb:
    def __init__(self, target):
        self.target = target

    def connect(self):
        """Connect, and restart adbd as root as tools/platform-probe.sh does: the kernel log and the open files of
        other processes need it. Without root the checks that need it say so."""
        subprocess.run(["adb", "connect", self.target], capture_output=True, text=True, timeout=20)
        subprocess.run(["adb", "-s", self.target, "root"], capture_output=True, text=True, timeout=20)
        time.sleep(2)
        subprocess.run(["adb", "connect", self.target], capture_output=True, text=True, timeout=20)

    def is_root(self):
        return self.sh("id -u").strip() == "0"

    def sh(self, cmd, timeout=60):
        r = subprocess.run(["adb", "-s", self.target, "shell", cmd], capture_output=True, text=True, timeout=timeout)
        return (r.stdout + r.stderr).replace("\r", "")

    def regs(self, addrs):
        return parse_reads(self.sh(remote_read_command(addrs)))


class Scpi:
    def __init__(self, host, port):
        self.s = socket.create_connection((host, port), timeout=5)
        self.buf = b""

    def write(self, cmd):
        self.s.sendall(cmd.encode() + b"\n")
        time.sleep(0.05)

    def query(self, cmd):
        """The answer line, or "" when none comes within the timeout (a command the scope does not know)."""
        self.write(cmd)
        while b"\n" not in self.buf:
            try:
                chunk = self.s.recv(4096)
            except socket.timeout:
                return ""
            if not chunk:
                break
            self.buf += chunk
        line, _, self.buf = self.buf.partition(b"\n")
        return line.decode(errors="replace").strip()

    def errors(self):
        out = []
        for _ in range(10):
            e = self.query(":SYSTem:ERRor?")
            if not e or e.startswith("0") or e.startswith("+0"):
                break
            out.append(e)
        return out


class Report:
    def __init__(self):
        self.results = []
        self.raw = []

    def add(self, key, title, status, detail):
        self.results.append((key, title, status, detail))
        print(f"  {status:4} {key:4} {title}: {detail}")

    def text(self, header):
        n = {s: sum(1 for r in self.results if r[2] == s) for s in ("PASS", "FAIL", "INFO", "SKIP")}
        lines = header + ["", "Summary: " + ", ".join(f"{v} {k}" for k, v in n.items()), ""]
        lines += [f"{s:4} {k:4} {t}: {d}" for k, t, s, d in self.results]
        lines += ["", "---- raw ----"] + self.raw
        return "\n".join(lines) + "\n"


def hx(v):
    return "----------" if v is None else f"0x{v:08x}"


def static_checks(adb, rep):
    hd = adb.sh("cat /sys/class/hdcode/hdcode_dev/hdcode_value").strip()
    code = int(hd) if hd.isdigit() else None
    rep.raw += ["hdcode: " + hd]
    rep.add("S1", "hardware code", "INFO" if code is not None else "FAIL", hd or "not readable")

    vt = adb.sh("cat /rigol/resource/conf/version.txt")
    fpga = adb.sh("ls /rigol/FPGA")
    rep.raw += ["version.txt:", vt, "ls /rigol/FPGA:", fpga]
    table = parse_versions(vt)
    differ = {c: n for c, n in table.items() if IMAGES.get(c) != n}
    if not table:
        rep.add("S2", "FPGA image per hardware code", "FAIL", "version.txt not readable")
    else:
        rep.add("S2", "FPGA image per hardware code", "PASS" if not differ else "FAIL",
                "as assumed" if not differ else f"differs for {differ}")
    if code is not None and table:
        img = table.get(code)
        present = img is not None and img in fpga.split()
        rep.add("S3", "image of this unit present", "PASS" if present else "FAIL", f"code {code} -> {img}")

    out = adb.sh("for g in 1 2 32 120; do echo \"$g $(cat /sys/class/gpio/gpio$g/direction 2>/dev/null) "
                 "$(cat /sys/class/gpio/gpio$g/value 2>/dev/null)\"; done")
    rep.raw += ["gpio:", out]
    seen = {int(p[0]): p[1:] for p in (l.split() for l in out.splitlines()) if p and p[0].isdigit()}
    for g, name, d, v in POWER_LINES:
        got = seen.get(g, [])
        if code == 1 and g in (1, 2):
            rep.add("S4", name, "INFO", f"{' '.join(got) or 'not exported'} (not driven on hardware code 1)")
            continue
        ok = len(got) >= 1 and got[0] == d and (v is None or (len(got) > 1 and got[1] == v))
        want = d if v is None else f"{d} {v}"
        rep.add("S4", name, "PASS" if ok else "FAIL", f"{' '.join(got) or 'not exported'} (expected {want})")

    lsmod = adb.sh("lsmod")
    rep.raw += ["lsmod:", lsmod]
    names = {l.split()[0] for l in lsmod.splitlines() if l.strip() and not l.startswith("Module")}
    missing = [m for m in MODULES if not any(n == m or n.startswith(m + "_") for n in names)]
    rep.add("S5", "drivers of the acquisition board loaded", "PASS" if not missing else "FAIL",
            "all" if not missing else "missing " + ", ".join(missing))

    ls = adb.sh("ls -l " + " ".join(DEVICES) + " 2>&1")
    spi = adb.sh("ls /dev/spidev* 2>/dev/null").split()
    rep.raw += ["devices:", ls, "spidev: " + " ".join(spi)]
    absent = [d for d in DEVICES if not re.search(r"\s" + re.escape(d) + r"\s*$", ls, re.M)]
    rep.add("S6", "device nodes", "PASS" if not absent else "FAIL",
            ("all" if not absent else "absent " + ", ".join(absent)) + f"; spidev: {' '.join(spi) or 'none'}")

    pci = adb.sh("for d in /sys/bus/pci/devices/*; do echo \"dev $d $(cat $d/vendor) $(cat $d/device)\"; "
                 "cat $d/resource; done")
    rep.raw += ["pci:", pci]
    found, sizes = False, []
    for block in pci.split("dev ")[1:]:
        head, _, res = block.partition("\n")
        f = head.split()
        if len(f) >= 3 and f[1] == "0x10ee" and f[2] == "0x7024":
            found, sizes = True, bar_sizes(res)
    rep.add("S7", "PCIe endpoint 10ee:7024", "PASS" if found else "FAIL", "present" if found else "not found")
    if found:
        big = max(sizes) if sizes else 0
        rep.add("S8", "register window of 16 MiB", "PASS" if big >= 16 << 20 else "FAIL",
                "BAR sizes " + ", ".join(f"{s >> 10} KiB" for s in sizes))

    clk = adb.sh("for f in /rigol/resource/conf/init_all_clock_V00_0*.txt; do echo \"$f $(grep -c . $f) "
                 "$(grep -cE '^R[0-9]+( \\(INIT\\))?[[:space:]]+0x[0-9A-Fa-f]{6}[[:space:]]*$' $f)\"; done")
    rep.raw += ["clock tables (file, lines, lines in TI format):", clk]
    rows = [l.split() for l in clk.splitlines() if l.startswith("/")]
    ok = bool(rows) and all(len(r) == 3 and r[1] == r[2] == "136" for r in rows)
    rep.add("S9", "clock tables: 136 TI register words each", "PASS" if ok else "FAIL",
            "; ".join(f"{r[0].rsplit('/', 1)[-1]} {r[1]}/{r[2] if len(r) > 2 else '?'}" for r in rows) or "none")

    users = adb.sh("for p in /proc/[0-9]*; do ls -l $p/fd 2>/dev/null | grep -oE "
                   "'xdma0_bypass|xdma0_c2h_0|spi_3wires|spi2k7_ctl' "
                   "| sed \"s|^|$(cat $p/comm 2>/dev/null) |\";"
                   " done | sort -u")
    rep.raw += ["open device nodes:", users]
    held = "; ".join(line for line in users.splitlines() if line.strip())
    rep.add("S10", "processes holding the devices", "INFO", held or "none seen (needs adb root)")

    log = adb.sh("dmesg 2>/dev/null | grep -iE 'PIN_DONE|PIN_INITB|PIN_PROGRAMB|xdma.*probe|10ee' | tail -20")
    rep.raw += ["dmesg:", log]
    rep.add("S11", "FPGA configured", *fpga_done(log))
    return code


def register_checks(adb, rep, code):
    r = adb.regs([a for a, _ in STATIC_REGS])
    for a, n in STATIC_REGS:
        rep.raw.append(f"reg 0x{a:05x} {hx(r.get(a))} {n}")
    if not r:
        rep.add("R0", "register reads", "FAIL", f"{PCIE2DEV} gave no value (is it there? is the app running?)")
        return False
    v = r.get(0x0004)
    rep.add("R1", "FPGA version 0x0004", "INFO" if v not in (None, 0, 0xFFFFFFFF) else "FAIL", hx(v))
    hv = r.get(0x401C)
    rep.add("R2", "hardware version 0x401c", "INFO", f"{hx(hv)} (hardware code {code})")
    dead = [f"0x{a:x}" for a, _ in STATIC_REGS if r.get(a) == 0xFFFFFFFF]
    rep.add("R3", "the blocks answer", "PASS" if not dead else "FAIL", "no 0xffffffff" if not dead else ", ".join(dead))
    ok, text = adc_bridge_word(r.get(0x3000, 0))
    rep.add("R4", "ADC SPI bridge word in 0x3000", "INFO" if ok is None else "PASS" if ok else "FAIL", text)
    return True


def mask_regs(adb):
    r = adb.regs([0x2438, 0x6110])
    return r.get(0x2438), r.get(0x6110)


def snap(adb, rep, label):
    r = adb.regs(SNAP_REGS)
    rep.raw.append(f"snapshot {label}: " + " ".join(f"{a:x}={hx(r.get(a))}" for a in SNAP_REGS))
    return r


def scpi_checks(adb, scpi, rep):
    idn = scpi.query("*IDN?")
    rep.raw.append("IDN: " + idn_without_serial(idn))
    scpi.errors()
    keep = {}
    for q in [":CHANnel1:DISPlay?", ":CHANnel2:DISPlay?", ":CHANnel3:DISPlay?", ":CHANnel4:DISPlay?",
              ":CHANnel1:SCALe?", ":TIMebase:MAIN:SCALe?", ":ACQuire:MDEPth?", ":WAVeform:FORMat?",
              ":WAVeform:SOURce?", ":WAVeform:MODE?", ":TRIGger:STATus?", ":MASK:ENABle?", ":MASK:OPERate?"]:
        keep[q] = scpi.query(q)
    rep.raw.append("settings before: " + "; ".join(f"{k} {v}" for k, v in keep.items()))

    def settle():
        time.sleep(1.5)

    def chans(on):
        for n in range(1, 5):
            scpi.write(f":CHANnel{n}:DISPlay {'ON' if n in on else 'OFF'}")

    try:
        scpi.write(":RUN")
        scpi.write(":TIMebase:MAIN:SCALe 1e-6")
        steps = [("R5", (1,), 0x1), ("R6", (1, 2), 0x3), ("R7", (1, 2, 3, 4), 0xF), ("R8", (1, 2, 3), None)]
        for key, on, want in steps:
            chans(on)
            settle()
            r = snap(adb, rep, f"channels {on}")
            m = r.get(0x4008)
            got = None if m is None else channel_mask(m)
            status = "INFO" if want is None or got is None else "PASS" if got == want else "FAIL"
            rep.add(key, f"0x4008[3:0] with CH{'+'.join(map(str, on))}", status,
                    f"{'----' if got is None else hex(got)}" + ("" if want is None else f" (expected {hex(want)})"))
            if m is not None and key == "R5":
                rep.add("R9", "roll and scan bits off at 1 us/div", "PASS" if not m & (1 << 17 | 1 << 22) else "FAIL",
                        f"0x4008 = {hx(m)}")

        chans((1,))
        depths = {}
        for d in ("1k", "1M"):
            scpi.write(f":ACQuire:MDEPth {d}")
            settle()
            r = snap(adb, rep, f"depth {d}")
            depths[d] = (r.get(0x10E4), scpi.query(":ACQuire:MDEPth?"), scpi.query(":ACQuire:SRATe?"))
        a, b = depths["1k"][0], depths["1M"][0]
        rep.add("R10", "record depth 0x10e4 follows the memory depth",
                "PASS" if a is not None and b is not None and a != b else "FAIL",
                f"1k: {hx(a)} ({depths['1k'][1]} pts, {depths['1k'][2]} Sa/s), "
                f"1M: {hx(b)} ({depths['1M'][1]} pts, {depths['1M'][2]} Sa/s)")
        if keep[":ACQuire:MDEPth?"]:
            scpi.write(f":ACQuire:MDEPth {depth_word(keep[':ACQuire:MDEPth?'])}")

        t0 = adb.regs([0x40B0, 0x40B4])
        time.sleep(0.5)
        t1 = adb.regs([0x40B0, 0x40B4])
        moved = "PASS" if t0 and t1 and t0 != t1 else "INFO"
        rep.add("R11", "frame time stamp 0x40b0/0x40b4 moves while running", moved,
                f"{hx(t0.get(0x40B4))}:{hx(t0.get(0x40B0))} -> {hx(t1.get(0x40B4))}:{hx(t1.get(0x40B0))}")

        scpi.write(":TIMebase:MAIN:SCALe 0.2")
        time.sleep(3)
        r = snap(adb, rep, "200 ms/div")
        m = r.get(0x4008)
        bits = None if m is None else (m >> 17 & 1, m >> 22 & 1)
        rep.add("R12", "roll (bit 17) or scan (bit 22) at 200 ms/div", "PASS" if bits and 1 in bits else "FAIL",
                f"0x4008 = {hx(m)}, roll {bits[0] if bits else '-'}, scan {bits[1] if bits else '-'}, "
                f":TIMebase:ROLL? {scpi.query(':TIMebase:ROLL?')}")
        scpi.write(":TIMebase:MAIN:SCALe 1e-6")

        scpi.write(":STOP")
        settle()
        r = snap(adb, rep, "stop")
        rep.add("R13", "registers in STOP", "INFO",
                f"0x4004 {hx(r.get(0x4004))}, 0x4034 {hx(r.get(0x4034))}, stop reason 0x4100 {hx(r.get(0x4100))}")
        scpi.write(":RUN")

        # the mask test switched on and started; no mask is created, so the owner's own mask stays as it is
        scpi.errors()
        before = mask_regs(adb)
        scpi.write(":MASK:ENABle ON")
        settle()
        enabled = mask_regs(adb)
        scpi.write(":MASK:OPERate RUN")
        settle()
        running = mask_regs(adb)
        err = scpi.errors()
        scpi.write(f":MASK:OPERate {keep[':MASK:OPERate?'] or 'STOP'}")
        scpi.write(f":MASK:ENABle {keep[':MASK:ENABle?'] or 'OFF'}")
        moved = all(None not in r for r in (before, enabled, running)) and len({before, enabled, running}) > 1
        rep.add("R14", "mask block (0x2438, 0x6110) follows the mask test", "SKIP" if err else
                "PASS" if moved else "INFO",
                "; ".join(f"{n} {hx(r[0])}/{hx(r[1])}" for n, r in (("off", before), ("on", enabled),
                                                                    ("running", running)))
                + (f" ({'; '.join(err)})" if err else
                "" if moved else " (unchanged: the test may need a mask of its own, which this check does not make)"))

        scpi.write(":WAVeform:SOURce CHANnel1")
        scpi.write(":WAVeform:MODE NORMal")
        scpi.write(":WAVeform:FORMat WORD")
        for s in ("0.1", "1"):
            scpi.write(f":CHANnel1:SCALe {s}")
            settle()
            scale = float(scpi.query(":CHANnel1:SCALe?") or s)
            pre = scpi.query(":WAVeform:PREamble?")
            rep.raw.append(f"preamble at {scale} V/div: {pre}")
            ok, text = preamble_check(pre, scale)
            rep.add("R15", f"sample words at {scale:g} V/div", "PASS" if ok else "FAIL", text)
    finally:
        restore(scpi, rep, keep)


# What is set back, in this order, and how the value read before is written again.
RESTORE = [":CHANnel1:DISPlay", ":CHANnel2:DISPlay", ":CHANnel3:DISPlay", ":CHANnel4:DISPlay", ":CHANnel1:SCALe",
           ":TIMebase:MAIN:SCALe", ":ACQuire:MDEPth", ":WAVeform:FORMat", ":WAVeform:SOURce", ":WAVeform:MODE",
           ":MASK:OPERate", ":MASK:ENABle"]
WRITE_AS = {":ACQuire:MDEPth": depth_word}


def same_setting(a, b):
    try:
        return abs(float(a) - float(b)) <= 1e-6 * max(abs(float(a)), abs(float(b)))
    except ValueError:
        return a.strip().upper() == b.strip().upper()


def restore(scpi, rep, keep):
    """Every setting written back one by one, each refusal named, then read again and compared with before."""
    scpi.errors()
    refused, differ = [], []
    for cmd in RESTORE:
        v = keep.get(cmd + "?")
        if not v:
            continue
        w = WRITE_AS.get(cmd, str)(v)
        scpi.write(f"{cmd} {w}")
        err = scpi.errors()
        if err:
            refused.append(f"{cmd} {w}: {'; '.join(err)}")
    scpi.write(":STOP" if keep.get(":TRIGger:STATus?", "").upper().startswith("STOP") else ":RUN")
    time.sleep(1.5)
    for cmd in RESTORE:
        v = keep.get(cmd + "?")
        if v:
            now = scpi.query(cmd + "?")
            if not same_setting(now, v):
                differ.append(f"{cmd} {now} (was {v})")
    ok = not refused and not differ
    rep.add("R16", "settings set back", "PASS" if ok else "FAIL",
            "all as before" if ok else "; ".join(refused + differ) + " - please set these back by hand")
    print("settings set back" if ok else "SETTINGS NOT ALL SET BACK, please set these by hand: "
          + "; ".join(refused + differ))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("host")
    ap.add_argument("--adb-port", type=int, default=55555)
    ap.add_argument("--scpi-port", type=int, default=5555)
    ap.add_argument("--no-scpi", action="store_true", help="read only; leave the scope's settings alone")
    ap.add_argument("--yes", action="store_true", help="do not ask before changing settings over SCPI")
    a = ap.parse_args()

    adb = Adb(f"{a.host}:{a.adb_port}")
    try:
        adb.connect()
    except FileNotFoundError:
        sys.exit("adb not found (Android platform tools)")
    if "sparrow3_" not in adb.sh("ls /rigol/FPGA"):
        sys.exit("this is not an MHO900 under its stock system (no sparrow3 image in /rigol/FPGA); nothing done")
    if "pcie2dev" not in adb.sh(f"ls {PCIE2DEV}"):
        sys.exit(f"{PCIE2DEV} is missing; nothing done")

    rep = Report()
    root = adb.is_root()
    rep.raw.append(f"adb root: {root}")
    print("adb root: " + ("yes" if root else "no (S10 and S11 may stay empty)"))
    print("system:")
    code = static_checks(adb, rep)
    print("registers (read only):")
    regs_ok = register_checks(adb, rep, code)

    idn = ""
    if a.no_scpi:
        rep.add("R5", "checks with changed settings", "SKIP", "--no-scpi")
    elif not regs_ok:
        rep.add("R5", "checks with changed settings", "SKIP", "no register reads")
    else:
        if not a.yes:
            ans = input("The next part changes channels, time base, memory depth, waveform format and the mask test "
                        "over SCPI for about a minute and sets them back. Go on? [y/N] ")
            if ans.strip().lower() not in ("y", "yes", "j", "ja"):
                rep.add("R5", "checks with changed settings", "SKIP", "declined")
                a.no_scpi = True
        if not a.no_scpi:
            print("settings and registers:")
            try:
                scpi = Scpi(a.host, a.scpi_port)
                idn = idn_without_serial(scpi.query("*IDN?"))
                scpi_checks(adb, scpi, rep)
            except OSError as e:
                rep.add("R5", "checks with changed settings", "SKIP", f"SCPI: {e}")

    model = (idn.split(",")[1].strip() if idn.count(",") >= 3 else "MHO900").replace(" ", "")
    name = f"mho-check-{model}-{datetime.datetime.now():%Y%m%d-%H%M%S}.txt"
    header = [f"mho-check {VERSION} (OOS tools/mho-check.py), {datetime.datetime.now():%Y-%m-%d %H:%M}",
              f"instrument: {idn or 'not asked (no SCPI)'}", f"hardware code: {code}",
              "Expectations: docs/protocols/mho-acquisition.md. PASS = as assumed, FAIL = differs, INFO = recorded."]
    with open(name, "w") as f:
        f.write(rep.text(header))
    print(f"written: {name} - please attach it to the GitHub issue")


if __name__ == "__main__":
    main()
