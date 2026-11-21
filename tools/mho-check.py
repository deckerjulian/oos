#!/usr/bin/env python3
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
"""Check on an MHO900 under its stock system what OOS assumes about its acquisition board.

  mho-check.py <scope-ip> [--adb-port 55555] [--scpi-port 5555] [--no-scpi] [--yes] [--map | --afe | --panel] [--fifty]

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

With --map it runs no checks but maps the front ends and the sample path: it steps through the vertical settings of
the four channels (scale, offset, coupling, bandwidth limit; with --fifty also 50 ohm), the time base and the
trigger level over SCPI, and after each step records what changed in the FPGA's configuration registers (only those
the stock system writes itself, read back), in the GPIO lines (/sys/kernel/debug/gpio: the front ends' select
lines), in the ADC bridge word, and in the kernel log. It writes mho-map-<model>-<date>.txt. Nothing may be connected
to the inputs; the settings are set back at the end as above.

With --afe it records, for the same steps and more (offset and AC coupling at several scales, every scale of CH2 to
CH4), the words the stock system sends to the front ends and the clock chip over their GPIO buses, decoded as front-end
register and data. It switches on the kernel's trace event gpio_value for those lines only (tracefs, in memory),
decodes the levels on the PC and switches the event off again, with the trace settings as they were; nothing else is
touched. It writes mho-afe-<model>-<date>.txt.

With --panel it records the front panel: the stock system's own traffic on its serial ports (the process that holds
/dev/ttyS0 open, followed with strace for its reads and writes, if the stock system has strace; it changes nothing),
while the owner presses each key and turns each knob and types its name on the PC. It writes
mho-panel-<model>-<date>.txt with the frames decoded (docs/protocols/frontpanel.md). The scope reacts slower while it
records. Without strace it records only the ports' byte counters.

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

VERSION = "6"
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


# ---- the front panel (--panel) ----

def panel_crc7(cmd, data):
    """CRC-7/MMC over [cmd, data] (docs/protocols/frontpanel.md)."""
    c = 0
    for b in (cmd & 0xff, data & 0xff):
        c ^= b
        for _ in range(8):
            c = ((c << 1) ^ 0x12) & 0xff if c & 0x80 else (c << 1) & 0xff
    return c >> 1


def panel_frames(data):
    """Splits bytes into the DHO's frames FA cmd data crc7 AF: [(cmd, data)] and the bytes that start none."""
    frames, skipped, i = [], [], 0
    while i < len(data):
        f = data[i:i + 5]
        if len(f) == 5 and f[0] == 0xfa and f[4] == 0xaf and f[3] == panel_crc7(f[1], f[2]):
            frames.append((f[1], f[2]))
            i += 5
        else:
            skipped.append(data[i])
            i += 1
    return frames, bytes(skipped)


STRACE_LINE = re.compile(r'^(\d+)\s+(\d+\.\d+)\s+(?:(read|write)\((\d+),\s*(?:"([^"]*)"|<unfinished)|<\.\.\. (read|write) '
                         r'resumed>\s*"([^"]*)")')


def parse_strace(text, fds):
    """The reads and writes of `strace -f -ttt -xx` on the descriptors `fds`: [(time, 'read'|'write', fd, bytes)].
    A call split by another thread (unfinished ... resumed) is joined; a read that returned nothing is left out."""
    out, pending = [], {}
    for line in text.splitlines():
        m = STRACE_LINE.match(line)
        if not m:
            continue
        pid, t = m.group(1), float(m.group(2))
        if m.group(3):
            call, fd, data = m.group(3), int(m.group(4)), m.group(5)
            if data is None:
                pending[pid] = (call, fd)
                continue
        else:
            call, data = m.group(6), m.group(7)
            if pid not in pending:
                continue
            fd = pending.pop(pid)[1]
        if fd in fds and data:
            out.append((t, call, fd, bytes.fromhex(data.replace("\\x", ""))))
    return out


def panel_key_names():
    """The DHO's key names (tools/frontpanel.py), or none when that file is not beside this one."""
    try:
        import importlib.util
        import pathlib
        spec = importlib.util.spec_from_file_location("frontpanel", pathlib.Path(__file__).with_name("frontpanel.py"))
        m = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(m)
        return m.KEYS
    except (OSError, AttributeError, ImportError):
        return {}


def panel_lines(calls, ports, labels, names):
    """The report body: every call in time order with its frames, the owner's labels between them."""
    out, li = [], 0
    for t, call, fd, data in calls:
        while li < len(labels) and labels[li][0] <= t:
            out.append(f"---- {labels[li][1]}")
            li += 1
        frames, skipped = panel_frames(data)
        way = "in " if call == "read" else "out"
        desc = [f"{c:02x} {d:02x}" + (f" ({names[d]})" if call == "read" and d in names else "") for c, d in frames]
        if skipped:
            desc.append("other bytes " + skipped.hex(" "))
        out.append(f"{t:.3f} {ports.get(fd, fd)} {way} " + "; ".join(desc))
    out += [f"---- {text}" for _, text in labels[li:]]
    return out


def panel_main(a, adb):
    if not adb.is_root():
        return "--panel needs adb root (it follows another process); nothing done"
    pids = adb.sh("for p in /proc/[0-9]*; do ls -l $p/fd 2>/dev/null | grep -q /dev/ttyS0 && echo ${p#/proc/}; done").split()
    pids = [p for p in pids if p.isdigit()]
    counters = lambda: adb.sh("cat /proc/tty/driver/serial").strip()
    idn = ""
    try:
        idn = idn_without_serial(Scpi(a.host, a.scpi_port).query("*IDN?"))
    except OSError:
        pass
    out = [f"mho-check {VERSION} --panel (OOS tools/mho-check.py), {datetime.datetime.now():%Y-%m-%d %H:%M}",
           f"instrument: {idn or 'not asked'}", f"kernel: {adb.sh('uname -r').strip()}", ""]
    if not pids:
        out.append("no process holds /dev/ttyS0 open")
    ports = {}
    for pid in pids[:1]:
        out.append(f"process with /dev/ttyS0: {pid} {adb.sh(f'cat /proc/{pid}/cmdline').split(chr(0))[0].strip()}")
        for line in adb.sh(f"ls -l /proc/{pid}/fd").splitlines():
            m = re.search(r"\s(\d+) -> (/dev/tty\S+)", line)
            if m:
                ports[int(m.group(1))] = m.group(2).replace("/dev/", "")
        out.append("its serial ports: " + ", ".join(f"fd {k} {v}" for k, v in sorted(ports.items())))
    out.append("ttyS0 settings: " + adb.sh("stty -F /dev/ttyS0 -a 2>&1 | head -3").strip().replace("\n", " | "))
    strace = adb.sh("which strace").strip()
    out += ["strace: " + (strace or "not on this system: only the byte counters are recorded"), "",
            "counters before:", counters(), ""]
    if not a.yes and input("--panel follows the stock program's serial ports while you press keys (the scope reacts "
                           "slower meanwhile). Go on? [y/N] ").strip().lower() not in ("y", "yes", "j", "ja"):
        return "declined; nothing done"
    proc = None
    log = "/data/local/tmp/oos-panel.strace"
    if strace and pids:
        tids = adb.sh(f"ls /proc/{pids[0]}/task").split()
        args = " ".join(f"-p {t}" for t in tids if t.isdigit())
        proc = subprocess.Popen(["adb", "-s", adb.target, "shell",
                                 f"rm -f {log}; strace -f -ttt -xx -s 256 -e trace=read,write -o {log} {args}"],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(3)
    offset = float(adb.sh("date +%s").strip() or 0) - time.time()
    print("Press one key or turn one knob a few steps, then type here what it was (e.g. 'Run/Stop', 'V-Scale CH1 "
          "right 3'). Every key and knob once, the power key last only briefly. An empty line ends.")
    labels = []
    while True:
        text = input("what was it: ").strip()
        if not text:
            break
        labels.append((time.time() + offset, text))
    if proc:
        adb.sh("kill -INT $(pidof strace)")
        proc.wait(timeout=20)
    out += ["counters after:", counters(), ""]
    if proc:
        text = adb.sh(f"cat {log}; rm -f {log}", timeout=120)
        calls = parse_strace(text, set(ports))
        out += ["Frames: time, port, in (from the panel) or out (to it), then cmd and data of each frame (a DHO key "
                "name where the data byte is one). The owner's labels follow the frames they name (scope time; the "
                "label is typed after the key, so its frames come before it).", ""]
        out += panel_lines(calls, ports, labels, panel_key_names())
    else:
        out += ["labels: " + "; ".join(t for _, t in labels)]
    model = (idn.split(",")[1].strip() if idn.count(",") >= 3 else "MHO900").replace(" ", "")
    name = f"mho-panel-{model}-{datetime.datetime.now():%Y%m%d-%H%M%S}.txt"
    with open(name, "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"written: {name} - please attach it to the GitHub issue")
    return 0


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


def restore(scpi, rep, keep, cmds=None):
    """Every setting written back one by one, each refusal named, then read again and compared with before. An entry
    (command, value) is only written, with that value, and not compared: a channel is switched on before its own
    settings go back, since the stock system does not take the scale of a channel that is off (seen on an MHO98)."""
    cmds = RESTORE if cmds is None else cmds
    scpi.errors()
    refused, differ = [], []
    for cmd in cmds:
        if isinstance(cmd, tuple):
            scpi.write(f"{cmd[0]} {cmd[1]}")
            scpi.errors()
            continue
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
    for cmd in cmds:
        if isinstance(cmd, tuple):
            continue
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


# ---- --map: what the stock system sets in the FPGA and on the GPIOs for each front-panel setting ----

# Configuration registers the stock system writes, read back after each step (it reads them only; reading back a
# setting has no effect on the FPGA). No status, counter or FIFO register is among them.
MAP_REGS = [
    (0x1004, "ADC order, channel filter"), (0x1014, "ADC bit range, calibration select"),
    (0x1058, "ADC gain fine"), (0x105c, "ADC gain"),
    (0x10d0, "sample rate 0"), (0x10d4, "sample rate 1"), (0x10d8, "sample rate 2"), (0x10dc, "anti-alias"),
    (0x10e0, "segment size"), (0x10e4, "record depth"),
    (0x1120, "delay coarse 0"), (0x1124, "delay coarse 1"), (0x1128, "delay fine 0"), (0x112c, "delay fine 1"),
    (0x1154, "interpolation enable"), (0x1158, "linear interpolation 1"), (0x115c, "linear interpolation 2"),
    (0x1160, "linear interpolation 3"), (0x1164, "linear interpolation 4"), (0x1168, "interpolation factor"),
    (0x1328, "acquisition delay"),
    (0x1408, "probe offset DAC CH1"), (0x140c, "probe offset DAC CH2"), (0x1410, "probe offset DAC CH3"),
    (0x1414, "probe offset DAC CH4"),
    (0x1418, "AFE offset CH1"), (0x141c, "AFE offset CH2"), (0x1420, "AFE offset CH3"), (0x1424, "AFE offset CH4"),
    (0x1428, "DAC polling"), (0x142c, "probe power"),
    # one gain and offset per ADC lane, not per channel: CH1 alone has its values on all four (MHO98, MHO934)
    (0x14c8, "analog gain lane 1"), (0x14d0, "analog offset lane 1"), (0x14d4, "analog gain lane 2"),
    (0x14dc, "analog offset lane 2"), (0x14e0, "analog gain lane 3"), (0x14e8, "analog offset lane 3"),
    (0x14ec, "analog gain lane 4"), (0x14f4, "analog offset lane 4"),
    (0x155c, "gain after the ADC A"), (0x1560, "offset 1 after the ADC A"), (0x1564, "offset 2 after the ADC A"),
    (0x1568, "gain after the ADC B"), (0x156c, "offset 1 after the ADC B"), (0x1570, "offset 2 after the ADC B"),
    (0x1574, "offset bypass and mode"),
    (0x2004, "trigger level A 0"), (0x2008, "trigger level B 0"), (0x200c, "trigger level A 1"),
    (0x2010, "trigger level B 1"), (0x201c, "trigger A 0"), (0x2020, "trigger A 1"), (0x2024, "trigger A 2"),
    (0x2028, "trigger A 3"), (0x20b8, "trigger level A 2"), (0x20bc, "trigger level B 2"),
    (0x20c0, "trigger level A 3"), (0x20c4, "trigger level B 3"), (0x2264, "external trigger hysteresis"),
    (0x4008, "SCU mode"), (0x4050, "pre-trigger samples"), (0x4054, "post-trigger samples"),
    (0x10100, "PWM 0"), (0x10108, "PWM 1"), (0x10120, "DAC sync"),
]
# How often the last ADC bridge word (0x3000) is read while a setting changes: it shows some of the ADC register
# writes the change makes (the data as written, with the stock system's mask applied).
ADC_POLLS = 120
ADC_SPI_WORD = 0x3000
MAC_RE = re.compile(r"\b[0-9a-fA-F]{2}(?::[0-9a-fA-F]{2}){5}\b")


# The scales of the front panel, 1 mV/div to 10 V/div (the stock system takes 500 uV/div as 1 mV/div).
SCALES = ("0.001", "0.002", "0.005", "0.01", "0.02", "0.05", "0.1", "0.2", "0.5", "1", "2", "5", "10")


def map_steps(fifty=False):
    """[(label, [SCPI commands])]: the steps of --map, in order. Every step starts from the state the one before left;
    the first sets the base: CH1 alone, DC, 1 MOhm, no bandwidth limit, no offset, 1 V/div, 1 us/div, edge trigger
    on CH1 at 0 V."""
    base = [":CHANnel1:DISPlay ON", ":CHANnel2:DISPlay OFF", ":CHANnel3:DISPlay OFF", ":CHANnel4:DISPlay OFF",
            ":CHANnel1:COUPling DC", ":CHANnel1:BWLimit OFF", ":CHANnel1:SCALe 1", ":CHANnel1:OFFSet 0",
            ":TIMebase:MAIN:SCALe 1e-6", ":TRIGger:MODE EDGE", ":TRIGger:EDGE:SOURce CHANnel1",
            ":TRIGger:EDGE:LEVel 0", ":RUN"]
    steps = [("base", base)]
    for v in SCALES:
        steps.append((f"CH1 {v} V/div", [f":CHANnel1:SCALe {v}"]))
    steps.append(("CH1 0.1 V/div", [":CHANnel1:SCALe 0.1"]))
    for o in ("-0.3", "0.3", "0"):
        steps.append((f"CH1 offset {o} V", [f":CHANnel1:OFFSet {o}"]))
    for c in ("AC", "GND", "DC"):
        steps.append((f"CH1 coupling {c}", [f":CHANnel1:COUPling {c}"]))
    for b in ("20M", "OFF"):
        steps.append((f"CH1 bandwidth limit {b}", [f":CHANnel1:BWLimit {b}"]))
    steps += fifty_steps(1) if fifty else []
    for n in (2, 3, 4):
        steps.append((f"CH{n} on, 1 V/div", [f":CHANnel{n}:DISPlay ON", f":CHANnel{n}:SCALe 1"]))
        steps.append((f"CH{n} 0.01 V/div", [f":CHANnel{n}:SCALe 0.01"]))
        steps += fifty_steps(n) if fifty else []
        steps.append((f"CH{n} off", [f":CHANnel{n}:DISPlay OFF"]))
    for t in ("2e-9", "1e-8", "1e-7", "1e-6", "1e-5", "1e-4", "1e-3"):
        steps.append((f"time base {t} s/div", [f":TIMebase:MAIN:SCALe {t}"]))
    steps.append(("time base 1e-6 s/div, CH1+2", [":TIMebase:MAIN:SCALe 1e-6", ":CHANnel2:DISPlay ON"]))
    steps.append(("CH1 to CH4", [":CHANnel3:DISPlay ON", ":CHANnel4:DISPlay ON"]))
    # with four channels each ADC lane carries one: which bits of 0x1004 and 0x1014 belong to which channel
    for n in (1, 2, 3, 4):
        steps += [(f"CH{n} bandwidth limit 20M, CH1 to CH4 on", [f":CHANnel{n}:BWLimit 20M"]),
                  (f"CH{n} bandwidth limit OFF, CH1 to CH4 on", [f":CHANnel{n}:BWLimit OFF"])]
    steps.append(("CH1 alone", [":CHANnel2:DISPlay OFF", ":CHANnel3:DISPlay OFF", ":CHANnel4:DISPlay OFF"]))
    for lv in ("-0.2", "0.2", "0"):
        steps.append((f"trigger level {lv} V at 0.1 V/div", [f":TRIGger:EDGE:LEVel {lv}"]))
    return steps


def fifty_steps(n):
    """Channel n at 50 ohm at three scales, then back at 1 MOhm and 0.1 V/div (--fifty; the input unconnected)."""
    steps = [(f"CH{n} 50 ohm {v} V/div", [f":CHANnel{n}:IMPedance FIFTy", f":CHANnel{n}:SCALe {v}"])
             for v in ("0.01", "0.1", "1")]
    return steps + [(f"CH{n} 1 MOhm", [f":CHANnel{n}:IMPedance OMEG", f":CHANnel{n}:SCALe 0.1"])]


def map_restore_list(fifty=False):
    """The settings --map and --afe change, in the order they are set back: each channel switched on first (a channel
    that is off does not take its scale), the impedance before the scale, the scale before the offset, the channel's
    own display state last, then the time base and the trigger."""
    out = []
    for n in range(1, 5):
        out.append((f":CHANnel{n}:DISPlay", "ON"))
        out += [f":CHANnel{n}:IMPedance"] if fifty else []
        out += [f":CHANnel{n}:COUPling", f":CHANnel{n}:BWLimit", f":CHANnel{n}:SCALe", f":CHANnel{n}:OFFSet",
                f":CHANnel{n}:DISPlay"]
    return out + [":TIMebase:MAIN:SCALe", ":TRIGger:MODE", ":TRIGger:EDGE:SOURce", ":TRIGger:EDGE:LEVel"]


def query_list(cmds):
    """The queries of a restore list: one per setting, not for the entries that are only written."""
    return [c for c in cmds if not isinstance(c, tuple)]


def gpio_lines(text):
    """{gpio number: "label dir value"} from /sys/kernel/debug/gpio (lines ` gpio-120 (label) in  lo`)."""
    out = {}
    for line in text.splitlines():
        m = re.match(r"\s*gpio-(\d+)\s+\((.*?)\)\s+(.*)$", line)
        if m:
            out[int(m.group(1))] = f"({m.group(2).strip()}) {' '.join(m.group(3).split())}"
    return out


def changes(before, after, fmt=str, key=str):
    """["key: old -> new"] for the keys whose value differs."""
    return [f"{key(k)}: {fmt(before.get(k))} -> {fmt(after.get(k))}"
            for k in sorted(set(before) | set(after)) if before.get(k) != after.get(k)]


def log_after(text, t0):
    """(lines of the kernel log after time t0, the last time stamp), MAC-like words masked."""
    new, last = [], t0
    for line in text.splitlines():
        m = LOG_RE.match(line.strip())
        if m and float(m.group(1)) > t0:
            new.append(MAC_RE.sub("<mac>", line.strip()))
            last = max(last, float(m.group(1)))
    return new, last


def map_state(adb):
    regs = adb.regs([a for a, _ in MAP_REGS])
    gpio = gpio_lines(adb.sh("cat /sys/kernel/debug/gpio"))
    return regs, gpio


def run_map(adb, scpi, out, fifty):
    """Each step of map_steps: the commands, the ADC bridge words seen meanwhile, then the registers, the GPIOs and
    the kernel log, as changes against the step before. `out` collects the lines of the report."""
    names = dict(MAP_REGS)
    keep = {}
    cmds = map_restore_list(fifty)
    for c in query_list(cmds) + [":TRIGger:STATus"]:
        keep[c + "?"] = scpi.query(c + "?")
    out.append("settings before: " + "; ".join(f"{k} {v}" for k, v in keep.items()))
    _, t0 = log_after(adb.sh("dmesg | tail -n 1"), 0.0)
    regs, gpio = map_state(adb)
    out += ["", "## registers before the first step"]
    out += [f"0x{a:05x} {hx(regs.get(a))} {n}" for a, n in MAP_REGS]
    out += ["", "## GPIOs before the first step"] + [f"gpio-{k} {v}" for k, v in sorted(gpio.items())]
    if not gpio:
        out.append("(none: /sys/kernel/debug/gpio not readable; adb root?)")
    table = []
    rep = Report()
    try:
        scpi.errors()
        for i, (label, commands) in enumerate(map_steps(fifty)):
            print(f"  {i:2} {label}")
            poll = subprocess.Popen(["adb", "-s", adb.target, "shell", remote_read_command([ADC_SPI_WORD] * ADC_POLLS)],
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            for c in commands:
                scpi.write(c)
            err = scpi.errors()
            words = []
            for w in READ_RE.findall(poll.communicate(timeout=120)[0]):
                v = int(w[1], 16)
                if not words or words[-1] != v:
                    words.append(v)
            time.sleep(1.0)
            r2, g2 = map_state(adb)
            log, t0 = log_after(adb.sh("dmesg | tail -n 40"), t0)
            now = {"V/div": scpi.query(":CHANnel1:SCALe?"), "s/div": scpi.query(":TIMebase:MAIN:SCALe?"),
                   "Sa/s": scpi.query(":ACQuire:SRATe?")}
            out += ["", f"## step {i}: {label}", "commands: " + "; ".join(commands),
                    "now: " + ", ".join(f"{k} {v}" for k, v in now.items())]
            if err:
                out.append("SCPI errors: " + "; ".join(err))
            out += ["register " + c + f"  ({names.get(int(c.split(':')[0], 16), '')})"
                    for c in changes(regs, r2, hx, lambda a: f"0x{a:05x}")]
            out += ["gpio-" + c for c in changes(gpio, g2)]
            if len(words) > 1:
                out.append("ADC bridge words: " + " ".join(f"{w:08x}" for w in words))
            out += ["kernel: " + line for line in log]
            table.append(f"{i};{label};" + ";".join(f"{r2.get(a, 0):x}" for a, _ in MAP_REGS))
            regs, gpio = r2, g2
    finally:
        restore(scpi, rep, keep, cmds)
        out += ["", "## settings set back"] + [f"{s} {k} {t}: {d}" for k, t, s, d in rep.results]
    out += ["", "## registers per step (hex)", "step;label;" + ";".join(f"{a:x}" for a, _ in MAP_REGS)] + table


def map_main(a, adb):
    if not adb.is_root():
        return "--map needs adb root (the GPIO states and the kernel log); nothing done"
    if not a.yes:
        ans = input("--map changes the vertical settings of CH1 to CH4, the time base and the trigger level over SCPI "
                    "for about 3 minutes and sets them back. Nothing may be connected to the inputs"
                    + (", and each channel goes to 50 ohm" if a.fifty else "") + ". Go on? [y/N] ")
        if ans.strip().lower() not in ("y", "yes", "j", "ja"):
            return "declined; nothing done"
    scpi = Scpi(a.host, a.scpi_port)
    idn = idn_without_serial(scpi.query("*IDN?"))
    code = adb.sh("cat /sys/class/hdcode/hdcode_dev/hdcode_value").strip()
    out = [f"mho-check {VERSION} --map (OOS tools/mho-check.py), {datetime.datetime.now():%Y-%m-%d %H:%M}",
           f"instrument: {idn}", f"hardware code: {code}", f"FPGA version: {hx(adb.regs([0x0004]).get(0x0004))}",
           "Each step lists what changed against the step before: configuration registers, GPIO lines, words seen in "
           "the ADC SPI bridge (0x3000) while the setting changed, and kernel log lines.", ""]
    print("steps:")
    run_map(adb, scpi, out, a.fifty)
    model = (idn.split(",")[1].strip() if idn.count(",") >= 3 else "MHO900").replace(" ", "")
    name = f"mho-map-{model}-{datetime.datetime.now():%Y%m%d-%H%M%S}.txt"
    with open(name, "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"written: {name} - please attach it to the GitHub issue")
    return 0


# ---- --afe: the words on the GPIO buses of the front ends, from the kernel's gpio_value trace event ----

# The front ends are set over a bus the processor drives on GPIOs (driver spi2afe_gpio, /dev/spi_simulate), so --map
# sees only where its data line was left (MHO98: AC coupling and the offset changed no register, only that line). The
# kernel's trace event gpio_value records every level the driver sets and reads on these lines; the words are decoded
# from it on the PC. Line numbers and names: /sys/kernel/debug/gpio of the platform probes (MHO954, MHO98).
TRACEFS = "/sys/kernel/debug/tracing"
AFE_BUS = {"name": "AFE", "sclk": 75, "mosi": 74, "miso": 73, "cs": {76: "AFE cs0", 66: "AFE cs1", 69: "AFE cs2",
                                                                      68: "AFE cs3"}}
# The channel behind each chip select, and the frame: 24 bits MSB first, valid at the rising clock edge (the driver sets
# the data line, then the clock low and high), register address in bits 23:16, data in bits 15:0 (MHO934 --afe).
AFE_CHANNEL = {"AFE cs3": "CH1", "AFE cs2": "CH2", "AFE cs1": "CH3", "AFE cs0": "CH4"}
AFE_FRAME_BITS = 24
# The clock chip's 3-wire bus (/dev/spi_3wires): one data line for both directions.
PLL_BUS = {"name": "PLL", "sclk": 49, "mosi": 51, "miso": 51, "cs": {50: "PLL cs"}}
# Select lines `a`, `b`, `c` in four groups (assumed: one group per channel), and the clock chip's other lines.
SELECT_LINES = {122: "a 122", 123: "b 123", 124: "c 124", 125: "a 125", 126: "b 126", 127: "c 127",
                129: "a 129", 130: "b 130", 131: "c 131", 149: "a 149", 152: "b 152", 153: "c 153",
                46: "vsel 46", 55: "vsel 55", 52: "reset 52", 54: "sysref_reset 54"}
TRACE_RE = re.compile(r"\s(\d+\.\d+):\s+gpio_value:\s+(\d+)\s+(set|get)\s+(\d+)")
RAW_PER_STEP = 800


def traced_lines():
    bus = [AFE_BUS, PLL_BUS]
    nums = {b[k] for b in bus for k in ("sclk", "mosi", "miso")} | {c for b in bus for c in b["cs"]}
    return sorted(nums | set(SELECT_LINES))


def trace_filter(lines):
    """The event filter that keeps only `lines` (the kernel's filter syntax of 4.4)."""
    return " || ".join(f"gpio == {n}" for n in lines)


def parse_trace(text):
    """[(time s, gpio, "set" or "get", value)] from the trace buffer, in order."""
    return [(float(t), int(g), op, int(v)) for t, g, op, v in TRACE_RE.findall(text)]


def bits_hex(bits):
    if not bits:
        return "-"
    return f"0x{int(''.join(map(str, bits)), 2):0{(len(bits) + 3) // 4}x}"


def decode_bus(events, bus, levels=None):
    """The frames on one bus: [(time, chip select, bits, data on rising clock, on falling clock, data read)]. A frame
    is the time a chip-select line is low; the data line is taken at each clock edge the driver sets (both edges, as
    the clock phase is not known), and what it reads at its `get`s of the input line. `levels` gives the lines' state
    before the first event (clock low, chip selects high when not given)."""
    lv = {bus["sclk"]: 0, bus["mosi"]: 0, **{c: 1 for c in bus["cs"]}}
    lv.update(levels or {})
    frames, cur = [], None
    for t, g, op, v in events:
        if op == "get":
            if g == bus["miso"] and cur is not None:
                cur["miso"].append(v)
            continue
        if g in bus["cs"]:
            if v == 0 and lv.get(g) != 0:
                cur = {"t": t, "cs": bus["cs"][g], "rise": [], "fall": [], "miso": []}
            elif v == 1 and lv.get(g) == 0 and cur is not None and cur["cs"] == bus["cs"][g]:
                frames.append(cur)
                cur = None
        elif g == bus["sclk"] and cur is not None and v != lv.get(g):
            cur["rise" if v else "fall"].append(lv.get(bus["mosi"], 0))
        lv[g] = v
    if cur is not None:
        frames.append(cur)
    return [(f["t"], f["cs"], len(f["rise"]), bits_hex(f["rise"]), bits_hex(f["fall"]), bits_hex(f["miso"]))
            for f in frames]


def afe_register(cs, n, rise):
    """(channel, register, data) of a front-end frame, None for a frame of another length or bus."""
    if cs not in AFE_CHANNEL or n != AFE_FRAME_BITS or rise == "-":
        return None
    w = int(rise, 16)
    return AFE_CHANNEL[cs], w >> 16, w & 0xFFFF


def select_changes(events, levels):
    """[(time, line name, old, new)] for the select lines whose level the driver changed."""
    out, lv = [], dict(levels)
    for t, g, op, v in events:
        if op == "set" and g in SELECT_LINES and lv.get(g) != v:
            out.append((t, SELECT_LINES[g], lv.get(g), v))
            lv[g] = v
    return out


def afe_steps(fifty=False):
    """The steps of --map, then what --map could not see: the offset of CH1 at two more scales, AC coupling at two
    scales, and every scale, offset, coupling and bandwidth limit of CH2 to CH4 (each switched on, CH1 stays on)."""
    steps = [("idle (nothing changed: what the stock system writes by itself)", [])] + map_steps(fifty)
    for v, o in (("0.01", "0.05"), ("1", "2")):
        steps += [(f"CH1 {v} V/div", [f":CHANnel1:SCALe {v}"]),
                  (f"CH1 offset {o} V at {v} V/div", [f":CHANnel1:OFFSet {o}"]),
                  (f"CH1 offset 0 V at {v} V/div", [":CHANnel1:OFFSet 0"]),
                  (f"CH1 coupling AC at {v} V/div", [":CHANnel1:COUPling AC"]),
                  (f"CH1 coupling DC at {v} V/div", [":CHANnel1:COUPling DC"])]
    steps.append(("CH1 0.1 V/div", [":CHANnel1:SCALe 0.1"]))
    for n in (2, 3, 4):
        steps.append((f"CH{n} on, 1 V/div", [f":CHANnel{n}:DISPlay ON", f":CHANnel{n}:SCALe 1"]))
        steps += [(f"CH{n} {v} V/div", [f":CHANnel{n}:SCALe {v}"]) for v in SCALES]
        steps += [(f"CH{n} 0.1 V/div", [f":CHANnel{n}:SCALe 0.1"]),
                  (f"CH{n} offset 0.3 V", [f":CHANnel{n}:OFFSet 0.3"]),
                  (f"CH{n} offset 0 V", [f":CHANnel{n}:OFFSet 0"]),
                  (f"CH{n} coupling AC", [f":CHANnel{n}:COUPling AC"]),
                  (f"CH{n} coupling DC", [f":CHANnel{n}:COUPling DC"]),
                  (f"CH{n} bandwidth limit 20M", [f":CHANnel{n}:BWLimit 20M"]),
                  (f"CH{n} bandwidth limit OFF", [f":CHANnel{n}:BWLimit OFF"])]
        steps += fifty_steps(n) if fifty else []
        steps.append((f"CH{n} off", [f":CHANnel{n}:DISPlay OFF"]))
    return steps


class Tracer:
    """The kernel's gpio_value event on the traced lines, switched on for --afe and set back as it was. It writes only
    to the trace control files of the kernel (no file, partition or register); the buffer lives in memory."""
    FILES = ["tracing_on", "buffer_size_kb", "events/gpio/gpio_value/enable"]

    def __init__(self, adb):
        self.adb, self.saved = adb, {}

    def available(self):
        """None when the event can be used, else why not."""
        enable = f"{TRACEFS}/events/gpio/gpio_value/enable"
        if self.adb.sh(f"ls {enable}").strip() != enable:
            return "this kernel has no gpio_value trace event (or tracefs is not mounted)"
        tracer = self.adb.sh(f"cat {TRACEFS}/current_tracer").strip()
        events = self.adb.sh(f"cat {TRACEFS}/set_event").strip()
        if tracer != "nop" or events:
            return f"the kernel's tracing is in use (tracer {tracer!r}, events {events!r}); left alone"
        return None

    def start(self):
        for f in self.FILES:
            self.saved[f] = self.adb.sh(f"cat {TRACEFS}/{f}").strip().split()[0]
        self.adb.sh(f"echo 0 > {TRACEFS}/tracing_on; echo 4096 > {TRACEFS}/buffer_size_kb; "
                    f"echo '{trace_filter(traced_lines())}' > {TRACEFS}/events/gpio/gpio_value/filter; "
                    f"echo 1 > {TRACEFS}/events/gpio/gpio_value/enable; echo > {TRACEFS}/trace")

    def record(self, action, seconds=1.5):
        """The events while `action` runs and `seconds` after it."""
        self.adb.sh(f"echo > {TRACEFS}/trace; echo 1 > {TRACEFS}/tracing_on")
        try:
            action()
            time.sleep(seconds)
        finally:
            self.adb.sh(f"echo 0 > {TRACEFS}/tracing_on")
        return self.adb.sh(f"cat {TRACEFS}/trace", timeout=120)

    def stop(self):
        """Everything as it was: the event off, its filter cleared, buffer size and tracing_on as before, buffer
        empty. Returns what could not be set back."""
        e = self.saved.get("events/gpio/gpio_value/enable", "0")
        self.adb.sh(f"echo 0 > {TRACEFS}/tracing_on; echo {e} > {TRACEFS}/events/gpio/gpio_value/enable; "
                    f"echo 0 > {TRACEFS}/events/gpio/gpio_value/filter; echo > {TRACEFS}/trace; "
                    f"echo {self.saved.get('buffer_size_kb', '1408')} > {TRACEFS}/buffer_size_kb; "
                    f"echo {self.saved.get('tracing_on', '1')} > {TRACEFS}/tracing_on")
        left = []
        for f, v in self.saved.items():
            now = self.adb.sh(f"cat {TRACEFS}/{f}").strip().split()
            if not now or now[0] != v:
                left.append(f"{TRACEFS}/{f} is {now[0] if now else '?'} (was {v})")
        return left


def run_afe(adb, scpi, out, fifty):
    tracer = Tracer(adb)
    why = tracer.available()
    if why:
        out += ["", f"not recorded: {why}"]
        print(why)
        return
    cmds = map_restore_list(fifty)
    keep = {c + "?": scpi.query(c + "?") for c in query_list(cmds) + [":TRIGger:STATus"]}
    out.append("settings before: " + "; ".join(f"{k} {v}" for k, v in keep.items()))
    levels = {}
    for g, line in gpio_lines(adb.sh("cat /sys/kernel/debug/gpio")).items():
        if g in traced_lines():
            levels[g] = 1 if line.split()[-1] == "hi" else 0
    out += ["", "lines traced (level before the first step): "
            + ", ".join(f"{g} {levels.get(g, '?')}" for g in traced_lines())]
    rep = Report()
    raw = []
    regs, table = {}, []
    tracer.start()
    try:
        scpi.errors()
        for i, (label, commands) in enumerate(afe_steps(fifty)):
            print(f"  {i:2} {label}")

            def act():
                for c in commands:
                    scpi.write(c)
            ev = parse_trace(tracer.record(act))
            err = scpi.errors()
            out += ["", f"## step {i}: {label}", "commands: " + ("; ".join(commands) or "none"),
                    f"events: {len(ev)}"]
            if err:
                out.append("SCPI errors: " + "; ".join(err))
            before = dict(regs)
            for bus in (AFE_BUS, PLL_BUS):
                for t, cs, n, rise, fall, miso in decode_bus(ev, bus, levels):
                    at = f"{cs}: +{(t - ev[0][0]) * 1e3:8.3f} ms  {n:2} clocks"
                    reg = afe_register(cs, n, rise)
                    if reg:
                        regs[reg[:2]] = reg[2]
                        out.append(f"{at}  {reg[0]} reg 0x{reg[1]:02x} = 0x{reg[2]:04x}"
                                   + (f"  read {miso}" if miso != "-" else ""))
                    else:
                        out.append(f"{at}  rise {rise}  fall {fall}" + (f"  read {miso}" if miso != "-" else ""))
            changed = [f"{c} 0x{r:02x} " + (f"{before[(c, r)]:04x}" if (c, r) in before else "-") + f" -> {d:04x}"
                       for (c, r), d in sorted(regs.items()) if before.get((c, r)) != d]
            if changed:
                out.append("front-end registers changed: " + ", ".join(changed))
            out += [f"line {name}: {old} -> {new} at +{(t - ev[0][0]) * 1e3:.3f} ms"
                    for t, name, old, new in select_changes(ev, levels)]
            for _, g, op, v in ev:
                if op == "set":
                    levels[g] = v
            table.append((i, label, dict(regs)))
            raw += ["", f"## raw, step {i}: {label} ({len(ev)} events"
                    + (f", the first {RAW_PER_STEP}" if len(ev) > RAW_PER_STEP else "") + ")"]
            raw += [f"{(t - ev[0][0]) * 1e6:10.1f} us {g:3} {op} {v}" for t, g, op, v in ev[:RAW_PER_STEP]]
    finally:
        left = tracer.stop()
        restore(scpi, rep, keep, cmds)
        out += ["", "## set back"] + [f"{s} {k} {t}: {d}" for k, t, s, d in rep.results]
        out.append("trace settings: " + ("as before" if not left else "NOT ALL SET BACK: " + "; ".join(left)))
        if left:
            print("trace settings not all set back (a restart of the scope sets them back): " + "; ".join(left))
    out += afe_table(table) + raw


def afe_table(rows):
    """The front-end registers after each step as a table, one column per channel and register ever written."""
    cols = sorted({k for _, _, r in rows for k in r})
    if not cols:
        return []
    out = ["", "## front-end registers per step (hex, - not written yet)",
           "step;label;" + ";".join(f"{c} 0x{r:02x}" for c, r in cols)]
    return out + [f"{i};{label};" + ";".join(f"{r[k]:04x}" if k in r else "-" for k in cols) for i, label, r in rows]


def afe_main(a, adb):
    if not adb.is_root():
        return "--afe needs adb root (the kernel's trace event); nothing done"
    if not a.yes:
        ans = input("--afe records the front ends' GPIO buses with the kernel's trace event while it changes the "
                    "vertical settings of CH1 to CH4, the time base and the trigger level over SCPI (about 7 minutes) "
                    "and sets them back. Nothing may be connected to the inputs"
                    + (", and each channel goes to 50 ohm" if a.fifty else "") + ". Go on? [y/N] ")
        if ans.strip().lower() not in ("y", "yes", "j", "ja"):
            return "declined; nothing done"
    scpi = Scpi(a.host, a.scpi_port)
    idn = idn_without_serial(scpi.query("*IDN?"))
    code = adb.sh("cat /sys/class/hdcode/hdcode_dev/hdcode_value").strip()
    out = [f"mho-check {VERSION} --afe (OOS tools/mho-check.py), {datetime.datetime.now():%Y-%m-%d %H:%M}",
           f"instrument: {idn}", f"hardware code: {code}", f"kernel: {adb.sh('uname -r').strip()}",
           "Each step lists the frames on the front ends' bus (AFE cs3 to cs0: CH1 to CH4) decoded as register and "
           "data, the frames on the clock chip's bus (PLL) with the data line at the rising and at the falling clock "
           "edges, the select lines that changed and the front-end registers that changed. A table of the front-end "
           "registers per step and the raw events follow at the end.", ""]
    print("steps:")
    run_afe(adb, scpi, out, a.fifty)
    model = (idn.split(",")[1].strip() if idn.count(",") >= 3 else "MHO900").replace(" ", "")
    name = f"mho-afe-{model}-{datetime.datetime.now():%Y%m%d-%H%M%S}.txt"
    with open(name, "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"written: {name} - please attach it to the GitHub issue")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("host")
    ap.add_argument("--adb-port", type=int, default=55555)
    ap.add_argument("--scpi-port", type=int, default=5555)
    ap.add_argument("--no-scpi", action="store_true", help="read only; leave the scope's settings alone")
    ap.add_argument("--yes", action="store_true", help="do not ask before changing settings over SCPI")
    ap.add_argument("--map", action="store_true",
                    help="instead of the checks: step through vertical, time base and trigger settings and record "
                         "what the stock system sets for each (about 3 minutes; nothing connected to the inputs)")
    ap.add_argument("--afe", action="store_true",
                    help="instead of the checks: record the words on the front ends' GPIO bus for each vertical "
                         "setting, with the kernel's gpio_value trace event (about 7 minutes; nothing connected)")
    ap.add_argument("--panel", action="store_true",
                    help="instead of the checks: record the stock system's front-panel traffic while you press each "
                         "key and type its name (follows the stock program with strace; changes nothing)")
    ap.add_argument("--fifty", action="store_true",
                    help="with --map or --afe: each channel at 50 ohm too (the inputs must be unconnected)")
    a = ap.parse_args()

    adb = Adb(f"{a.host}:{a.adb_port}")
    try:
        adb.connect()
    except FileNotFoundError:
        sys.exit("adb not found (Android platform tools)")
    if "sparrow3_" not in adb.sh("ls /rigol/FPGA"):
        sys.exit("this is not an MHO900 under its stock system (no sparrow3 image in /rigol/FPGA); nothing done")
    if a.panel:
        sys.exit(panel_main(a, adb))
    if "pcie2dev" not in adb.sh(f"ls {PCIE2DEV}"):
        sys.exit(f"{PCIE2DEV} is missing; nothing done")

    if a.afe:
        sys.exit(afe_main(a, adb))
    if a.map:
        sys.exit(map_main(a, adb))

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
