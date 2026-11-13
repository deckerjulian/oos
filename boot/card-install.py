#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""card-install.py: put an OOS boot image into the `recovery` partition of a DHO800/DHO900 boot card
(card in a reader as /dev/rdiskN, or a card image file), and optionally arm the boot command.

    card-install.py install TARGET --image boot/out/oos-dev.img [--arm]    dry run: checks and prints the plan
    card-install.py restore TARGET --backup FILE                            dry run: puts a saved recovery back
    add --execute to write. Before the first write the current `recovery` partition is saved to
    backups/recovery-<date>.bin in this repository (refuses to continue if that fails).

Checks before anything is written:
  - the PARM table at LBA 8192 has the stock layout (recovery 0x00020000@0x0002C000, misc 0x00002000@0x00006000)
  - the image is an ANDROID! boot image whose loader id holds (oos-bootimg.py) and fits the partition
  - the current recovery content is a KRNL ramdisk, an ANDROID! image, or a saved OOS image
--arm also writes `boot-recovery` to LBA 32800 (bcb.py), so the next power-on boots the image once
(the OOS development initramfs clears it again at start).
"""
import argparse, datetime, hashlib, importlib.util, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardconf

SECTOR = 512
BOARD = boardconf.load()
PARM_LBA = int(BOARD["OOS_PARM_LBA"])
RECOVERY_LBA, RECOVERY_SECTORS = int(BOARD["OOS_RECOVERY_LBA"]), int(BOARD["OOS_IMAGE_SECTORS"])
BOOT_LBA, BOOT_SECTORS = int(BOARD["OOS_BOOT_LBA"]), int(BOARD["OOS_BOOT_SECTORS"])
BACKUP_LBA = int(BOARD["OOS_BACKUP_LBA"])


def read(f, lba, n):
    f.seek(lba * SECTOR)
    out = b""
    while len(out) < n * SECTOR:
        c = f.read(min(n * SECTOR - len(out), 4 << 20))
        if not c:
            break
        out += c
    return out


def check_parm(f):
    raw = read(f, PARM_LBA, 4)
    if raw[:4] != b"PARM":
        sys.exit(f"no PARM block at LBA {PARM_LBA}: not a boot card of this board")
    txt = raw.decode("latin1")
    for need in ("0x00020000@0x0002C000(recovery)", "0x00002000@0x00006000(misc)"):
        if need.lower() not in txt.lower():
            sys.exit(f"partition table differs from the stock layout (missing {need}); refusing")
    print("PARM table: stock layout (recovery at LBA 0x2e000, misc at LBA 0x8000)")


def kind(blob):
    if blob[:4] == b"KRNL":
        return "KRNL image (stock recovery ramdisk)"
    if blob[:8] == b"ANDROID!":
        name = blob[48:64].split(b"\0")[0].decode("latin1")
        return f"ANDROID! boot image, name {name!r}"
    if blob.count(0) == len(blob):
        return "zeros"
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["install", "restore"])
    ap.add_argument("target")
    ap.add_argument("--image")
    ap.add_argument("--backup")
    ap.add_argument("--arm", action="store_true")
    ap.add_argument("--fallback-stock", action="store_true",
                    help="copy `boot` (stock Android ramdisk) into the first 32 MiB of an EMPTY `backup`, so a rejected recovery image falls back to Android")
    ap.add_argument("--execute", action="store_true")
    ap.add_argument("--backup-dir", default=os.path.join(HERE, "..", "backups"))
    a = ap.parse_args()
    spec = importlib.util.spec_from_file_location("oosbootimg", os.path.join(HERE, "oos-bootimg.py"))
    bi = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bi)

    src = a.image if a.action == "install" else a.backup
    if not src:
        sys.exit("give --image (install) or --backup (restore)")
    data = open(src, "rb").read()
    if a.action == "install":
        f_, k, r, s = bi.parts(data)
        if data[bi.ID_OFFSET:bi.ID_OFFSET + 32] != bi.image_id(data[:bi.PAGE], k, r, s):
            sys.exit(f"{src}: loader id does not hold; the loader would refuse it")
        print(f"image {src}: {len(data)} bytes, id ok, sha256 {hashlib.sha256(data).hexdigest()[:16]}")
    if len(data) > RECOVERY_SECTORS * SECTOR:
        sys.exit("does not fit `recovery` (64 MiB)")

    with open(a.target, "rb", buffering=0) as f:
        check_parm(f)
        cur = read(f, RECOVERY_LBA, RECOVERY_SECTORS)
        stock_boot = read(f, BOOT_LBA, BOOT_SECTORS) if a.fallback_stock else None
        backup_head = read(f, BACKUP_LBA, BOOT_SECTORS) if a.fallback_stock else None
    what = kind(cur[:4096])
    print(f"recovery now: {what or 'UNKNOWN content'}")
    if what is None:
        sys.exit("unexpected content in recovery; refusing (inspect it with tools/sdcard-layout.py)")
    plan = [f"write {len(data)} bytes to LBA {RECOVERY_LBA} (recovery)"]
    if a.fallback_stock:
        # OOS selected: `boot` differs from the copy only in its magic
        if stock_boot[:4] == b"OOSK" and backup_head[:4] == b"KRNL" and backup_head[4:] == stock_boot[4:]:
            print("backup already holds a copy of boot")
            a.fallback_stock = False
        elif stock_boot[:4] != b"KRNL":
            sys.exit("`boot` does not hold a KRNL image; --fallback-stock refused")
        elif backup_head == stock_boot:
            print("backup already holds a copy of boot")
            a.fallback_stock = False
        elif backup_head.count(0) != len(backup_head):
            sys.exit("`backup` is not empty; --fallback-stock refused (inspect it first)")
        else:
            plan.append(f"copy boot (32 MiB, stock Android ramdisk) to LBA {BACKUP_LBA} (backup), fallback if recovery is rejected")
    if a.arm:
        plan.append("write boot-recovery to LBA 32800 (next power-on boots recovery once)")
    for p in plan:
        print("plan:", p)
    if not a.execute:
        print("dry run: add --execute to write")
        return

    bdir = a.backup_dir
    os.makedirs(bdir, exist_ok=True)
    bfile = os.path.join(bdir, f"recovery-{datetime.datetime.now():%Y%m%d-%H%M%S}.bin")
    with open(bfile, "wb") as o:
        o.write(cur)
    if hashlib.sha256(open(bfile, "rb").read()).digest() != hashlib.sha256(cur).digest():
        sys.exit("backup of recovery failed; nothing written")
    print(f"saved current recovery to {os.path.abspath(bfile)}")

    padded = data + bytes(-len(data) % SECTOR)
    with open(a.target, "r+b", buffering=0) as f:
        f.seek(RECOVERY_LBA * SECTOR)
        f.write(padded)
        os.fsync(f.fileno())
        back = read(f, RECOVERY_LBA, len(padded) // SECTOR)
    if back != padded:
        sys.exit("READ-BACK MISMATCH in recovery; restore with: card-install.py restore TARGET --backup " + bfile)
    print("recovery written and verified")
    if a.fallback_stock:
        with open(a.target, "r+b", buffering=0) as f:
            f.seek(BACKUP_LBA * SECTOR)
            f.write(stock_boot)
            os.fsync(f.fileno())
            ok = read(f, BACKUP_LBA, BOOT_SECTORS) == stock_boot
        print("backup: stock boot copy written and verified" if ok else "backup: READ-BACK MISMATCH (recovery is fine)")
    if a.arm:
        sys.stdout.flush()
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "bcb.py"), "set", a.target, "recovery", "--execute"])


if __name__ == "__main__":
    main()
