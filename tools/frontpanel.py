#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""frontpanel.py: talk to the DHO800/DHO900 front-panel MCU over UART0 (protocol in docs/protocols/frontpanel.md).

Run it ON THE SCOPE under OOS (Python 3, no extra modules). Do not run it under stock Android: there the Rigol
Launcher reads /dev/ttyS0 and both readers would lose bytes.

    frontpanel.py dump                 print every frame (cmd, data) while you press keys and turn knobs
    frontpanel.py leds 0x0 0x0 0x0     set the LED bitmasks (LEDs 0-7, 8-15, 16-18)
    frontpanel.py selftest             CRC and framing self test (runs anywhere)
"""
import os, sys, termios, tty

DEV = "/dev/ttyS0"

# Key code = the frame's DATA byte (the stock reader ignores cmd). Release = data 0x00.
# Source: DHO900 user guide 4.4.1; "?" = not yet confirmed on the unit.
KEYS = {
    0x01: "Ref", 0x02: "LA", 0x03: "GI (AFG)", 0x04: "Horizontal Menu", 0x05: "Navigate",
    0x06: "Nav forward", 0x07: "Nav play/stop", 0x08: "Nav back", 0x09: "Trigger Slope", 0x0a: "Trigger Force",
    0x0b: "Clear", 0x0c: "Run/Stop", 0x0d: "Auto", 0x0e: "Single", 0x0f: "Measure", 0x10: "Cursor", 0x11: "Default",
    0x12: "Analyse", 0x13: "Touch Lock", 0x14: "Quick", 0x15: "CH1", 0x16: "CH2", 0x17: "CH3", 0x18: "CH4",
    0x19: "Math", 0x1a: "Trigger Menu", 0x1b: "Power",
    0x8a: "H-Position CW", 0x8b: "H-Position CCW", 0x8c: "H-Position push",
    0x9a: "V-Position CW", 0x9b: "V-Position CCW", 0x9c: "V-Position push",
    0xaa: "Multi 2 CW", 0xab: "Multi 2 CCW", 0xac: "Multi 2 push",
    0xba: "Multi 1 CW", 0xbb: "Multi 1 CCW", 0xbc: "Multi 1 push",
    0xca: "H-Scale CW", 0xcb: "H-Scale CCW", 0xcc: "H-Scale push",
    0xda: "V-Scale CW", 0xdb: "V-Scale CCW", 0xdc: "V-Scale push",
    0xea: "Trig Level CW", 0xeb: "Trig Level CCW", 0xec: "Trig Level push",
    0xf1: "CH1 overload", 0xf2: "CH2 overload", 0xf3: "CH3 overload", 0xf4: "CH4 overload",
}
BAUD = 1500000


def crc7(cmd, data):
    c = 0
    for b in (cmd & 0xff, data & 0xff):
        c ^= b
        for _ in range(8):
            c = ((c << 1) ^ 0x12) if c & 0x80 else (c << 1)
            c &= 0xfe
    return c >> 1


def frame(cmd, data):
    return bytes((0xFA, cmd & 0xff, data & 0xff, crc7(cmd, data), 0xAF))


def open_port():
    fd = os.open(DEV, os.O_RDWR | os.O_NOCTTY)
    tty.setraw(fd)
    attrs = termios.tcgetattr(fd)
    speed = getattr(termios, "B1500000", 0o010012)
    attrs[4] = attrs[5] = speed
    attrs[2] = (attrs[2] & ~(termios.PARENB | termios.CSTOPB | termios.CSIZE | termios.CRTSCTS)) | termios.CS8 | termios.CREAD | termios.CLOCAL
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def frames(fd):
    buf = bytearray()
    while True:
        buf += os.read(fd, 64)
        while len(buf) >= 5:
            if buf[0] != 0xFA:
                del buf[0]
                continue
            f = bytes(buf[:5])
            if f[4] != 0xAF or f[3] != crc7(f[1], f[2]):
                del buf[0]
                continue
            del buf[:5]
            yield f[1], f[2]


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "dump"
    if cmd == "decode":   # decode a hex dump: frontpanel.py decode fa 08 0c 34 af ...
        b = bytes(int(x, 16) for x in sys.argv[2:])
        for i in range(0, len(b) - 4, 5):
            f = b[i:i + 5]
            ok = f[0] == 0xFA and f[4] == 0xAF and f[3] == crc7(f[1], f[2])
            print(f"data {f[2]:#04x} cmd {f[1]:#04x}  {'release' if f[2] == 0 else KEYS.get(f[2], 'unknown')}{'' if ok else '  BAD FRAME'}")
        return
    if cmd == "selftest":
        for c, d in ((1, 0), (2, 0xff), (6, 0x15)):
            f = frame(c, d)
            print(f"cmd {c:#04x} data {d:#04x} -> {f.hex(' ')}")
        return
    fd = open_port()
    if cmd == "dump":
        print(f"{DEV} {BAUD} 8N1, press keys / turn knobs (Ctrl-C ends)")
        for c, d in frames(fd):
            name = "release" if d == 0 else KEYS.get(d, "unknown")
            print(f"data {d:#04x} cmd {c:#04x}  {name}", flush=True)
    elif cmd == "leds":
        masks = [int(x, 0) for x in sys.argv[2:5]]
        for i, m in enumerate(masks, 1):
            os.write(fd, frame(i, m))
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
