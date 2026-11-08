#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""bcb.py: read or set the boot command that Rigol's stock U-Boot (2014.10-RK3399) reads from `misc`.

Found in the stock U-Boot (uboot.bin, code at file offset 0x800 + 0x76b8): it looks up `misc`, reads 3 sectors at
misc start + 0x20 sectors (16 KiB) and compares the string at the start of that buffer with
    "boot-recovery"  -> bootrk recovery     (OOS boot image in `recovery`)
    "boot-factory"   -> bootrk factory
    "bootloader" / "loader" -> Rockchip Rockusb (download mode over USB)
anything else -> the normal `boot` path (stock Android). The AOSP-style text at misc offset 0 is not read.

Card layout: `misc` starts at LBA 32768 (PARM 0x2000@0x6000 + 0x2000), so the command lives at LBA 32800.

    bcb.py show   TARGET                    TARGET = card image, /dev/rdiskN (Mac), /dev/mmcblk0 or /dev/block/mmcblk0
    bcb.py set    TARGET recovery|loader    dry run: prints what would be written
    bcb.py clear  TARGET                    dry run
    add --execute to write; the sector is read back and compared. --misc-lba overrides 32768.
"""
import argparse, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import boardconf

SECTOR = 512
CMD_OFFSET_SECTORS = 0x20
MISC_LBA = int(boardconf.load()["OOS_BCB_LBA"]) - CMD_OFFSET_SECTORS
COMMANDS = {"recovery": b"boot-recovery", "factory": b"boot-factory", "loader": b"bootloader"}


def rw_open(path, write):
    return open(path, "r+b" if write else "rb", buffering=0)


def read_sector(f, lba):
    f.seek(lba * SECTOR)
    return f.read(SECTOR)


def describe(sec):
    cmd = sec[:32].split(b"\0")[0]
    if not cmd:
        return "empty -> normal boot (`boot`, stock Android)"
    for k, v in COMMANDS.items():
        if cmd == v:
            return f"{cmd.decode()!r} -> {'boots `recovery` (OOS)' if k == 'recovery' else k}"
    if cmd == b"loader":
        return "'loader' -> Rockusb"
    return f"{cmd!r} -> unknown to the stock U-Boot, normal boot"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["show", "set", "clear"])
    ap.add_argument("target")
    ap.add_argument("command", nargs="?", choices=list(COMMANDS))
    ap.add_argument("--misc-lba", type=int, default=MISC_LBA)
    ap.add_argument("--execute", action="store_true")
    a = ap.parse_args()
    lba = a.misc_lba + CMD_OFFSET_SECTORS
    with rw_open(a.target, False) as f:
        sec = read_sector(f, lba)
        head = read_sector(f, a.misc_lba)
    if len(sec) != SECTOR:
        sys.exit(f"cannot read LBA {lba} of {a.target}")
    print(f"{a.target}: misc at LBA {a.misc_lba}, command sector LBA {lba} (byte {lba * SECTOR:#x})")
    print(f"  now: {describe(sec)}")
    stale = head[:16].split(b"\0")[0]
    print(f"  (misc offset 0 holds {stale!r}; the stock U-Boot does not read it)")
    if a.action == "show":
        return
    if a.action == "set" and not a.command:
        sys.exit("set needs a command: recovery, factory or loader")
    new = bytearray(sec)
    new[:32] = bytes(32)
    if a.action == "set":
        new[:len(COMMANDS[a.command])] = COMMANDS[a.command]
    print(f"  new: {describe(bytes(new))}")
    if bytes(new) == sec:
        print("  unchanged, nothing to write")
        return
    if not a.execute:
        print("  dry run: add --execute to write this one sector")
        return
    with rw_open(a.target, True) as f:
        f.seek(lba * SECTOR)
        f.write(bytes(new))
        os.fsync(f.fileno())
        back = read_sector(f, lba)
    if back != bytes(new):
        sys.exit("  READ-BACK MISMATCH")
    print("  written and verified")


if __name__ == "__main__":
    main()
