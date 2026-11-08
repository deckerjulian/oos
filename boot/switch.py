#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""switch.py: choose what the stock Rigol loader boots on a DHO800/DHO900 card (card in the Mac reader or an image).

The stock loader (U-Boot 2014.10-RK3399, build 2024-08-29) boots `boot`; if that image fails its check it tries
`recovery` (OOS), then `backup` (a copy of the stock boot image, see card-install.py --fallback-stock).
It does NOT read the boot command in `misc` on a normal power-on (tested 2026-11-02).

    switch.py show  TARGET
    switch.py oos   TARGET [--execute]     "KRNL" at the start of `boot` (LBA 122880) -> "OOSK"
    switch.py stock TARGET [--execute]     back to "KRNL"; also re-enables a disabled fallback copy in `backup` ("BAKK" -> "KRNL")
`oos` refuses unless `recovery` holds an ANDROID! image and `backup` starts with a KRNL image (the stock fallback).
"""
import argparse, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import boardconf

SECTOR = 512
BOARD = boardconf.load()
PARM_LBA, BOOT_LBA, RECOVERY_LBA, BACKUP_LBA = (int(BOARD[k]) for k in ("OOS_PARM_LBA", "OOS_BOOT_LBA", "OOS_RECOVERY_LBA", "OOS_BACKUP_LBA"))


def rd(f, lba, n=1):
    f.seek(lba * SECTOR)
    return f.read(n * SECTOR)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["show", "oos", "stock"])
    ap.add_argument("target")
    ap.add_argument("--execute", action="store_true")
    a = ap.parse_args()
    with open(a.target, "rb", buffering=0) as f:
        parm, boot, rec, bak = rd(f, PARM_LBA, 2), rd(f, BOOT_LBA), rd(f, RECOVERY_LBA), rd(f, BACKUP_LBA)
    if parm[:4] != b"PARM" or b"0x00010000@0x0001C000(boot)".lower() not in parm.lower():
        sys.exit("not a stock-layout DHO card")
    state = {b"KRNL": "stock (boot valid)", b"OOSK": "OOS (boot disabled -> recovery)"}.get(boot[:4], f"unknown boot magic {boot[:4]!r}")
    print(f"boot: {state}; recovery: {rec[:8]!r}; backup: {bak[:4]!r}")
    if a.action == "show":
        return
    want = b"OOSK" if a.action == "oos" else b"KRNL"
    if boot[:4] not in (b"KRNL", b"OOSK"):
        sys.exit("unexpected boot content, refusing")
    if a.action == "oos" and (rec[:8] != b"ANDROID!" or bak[:4] != b"KRNL"):
        sys.exit("refusing: recovery must hold the OOS image and backup the stock fallback copy")
    fix_backup = a.action == "stock" and bak[:4] == b"BAKK"
    if boot[:4] == want and not fix_backup:
        print("already set")
        return
    writes = []
    if boot[:4] != want:
        writes.append((BOOT_LBA, want + boot[4:]))
    if fix_backup:
        writes.append((BACKUP_LBA, b"KRNL" + bak[4:]))
    if not a.execute:
        for lba, sec in writes:
            print(f"dry run: would write {sec[:4]!r} at LBA {lba}")
        print("add --execute")
        return
    with open(a.target, "r+b", buffering=0) as f:
        for lba, sec in writes:
            f.seek(lba * SECTOR)
            f.write(sec)
        os.fsync(f.fileno())
        ok = all(rd(f, lba) == sec for lba, sec in writes)
    print("written and verified" if ok else "READ-BACK MISMATCH")


if __name__ == "__main__":
    main()
