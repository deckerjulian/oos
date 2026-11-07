#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""sdcard-layout.py: describe the boot SD card of a Rigol DHO800/DHO900 (Rockchip RK3399 legacy layout).

Read-only. Input is a card image (sd-original.img), a raw device (/dev/rdiskN, needs sudo) or a partial dump.

Reports
  - LBA 0-63         MBR/GPT signatures
  - LBA 64           Rockchip IDB (TPL/SPL loader) present?
  - LBA 7168-7679    Rockchip vendor storage (4 copies of 64 KiB): items, serial/MAC
  - LBA 8192         Rockchip 'parameter' (PARM) block: CMDLINE mtdparts -> partition table
  - Android 'misc'   bootloader control block (BCB) command
  - each partition   first bytes / format guess (KRNL, RSCE, ANDROID!, ext4, zero)

Options
  --extract DIR      write every partition to DIR/<name>.bin (not userdata/rigol/system unless --all)
  --all              with --extract: also the big partitions
  --compare DIR      compare partitions with existing dumps DIR/<name>.bin (md5)

Partition sizes and offsets in the PARM block are in 512-byte sectors and RELATIVE to LBA 0x2000 (8192),
as Rockchip's U-Boot interprets them.
"""
import argparse, hashlib, os, re, struct, sys, zlib

SECTOR = 512
PARM_LBA = 0x2000
VENDOR_LBA, VENDOR_SECTORS, VENDOR_COPIES = 7168, 128, 4
VENDOR_TAG = 0x524B5644  # "DVKR" little endian
VENDOR_IDS = {1: "serial number", 2: "WiFi MAC", 3: "LAN MAC", 4: "BT MAC", 5: "HDCP 1.4 HDMI", 6: "HDCP 1.4 DP",
              7: "HDCP 2.x", 8: "DRM key", 9: "PlayReady cert", 10: "attentation key", 11: "PlayReady root"}


class Disk:
    def __init__(self, path):
        self.f = open(path, "rb", buffering=0)
        try:
            self.f.seek(0, 2)
            self.size = self.f.tell()
        except OSError:
            self.size = None
        if not self.size:  # raw devices on macOS report 0
            self.size = None

    def read(self, lba, count):
        # raw devices need sector-aligned reads; lba/count are in sectors
        self.f.seek(lba * SECTOR)
        out = b""
        want = count * SECTOR
        while len(out) < want:
            chunk = self.f.read(min(want - len(out), 4 << 20))
            if not chunk:
                break
            out += chunk
        return out


def show_head(d):
    mbr = d.read(0, 2)
    print("== LBA 0/1")
    print(f"   MBR signature 55AA: {'yes' if mbr[510:512] == b'\x55\xaa' else 'no'};  GPT header: {'yes' if mbr[512:520] == b'EFI PART' else 'no'}")
    idb = d.read(64, 4)
    print("== LBA 64 (IDB: TPL/SPL loader)")
    if not idb:
        print("   (beyond the end of the input)")
    elif idb.count(0) == len(idb):
        print("   empty")
    else:
        print(f"   present, first 16 bytes {idb[:16].hex(' ')} (RC4-scrambled RK header)")


def show_vendor(d):
    print(f"== LBA {VENDOR_LBA} vendor storage ({VENDOR_COPIES} copies x {VENDOR_SECTORS * SECTOR // 1024} KiB)")
    best = None
    for c in range(VENDOR_COPIES):
        blk = d.read(VENDOR_LBA + c * VENDOR_SECTORS, VENDOR_SECTORS)
        if len(blk) < 16:
            print(f"   copy {c}: beyond the end of the input")
            continue
        tag, version, next_index, item_num, free_offset, free_size = struct.unpack_from("<IIHHHH", blk, 0)
        version2 = struct.unpack_from("<I", blk, len(blk) - 4)[0]
        ok = tag == VENDOR_TAG and version == version2
        print(f"   copy {c}: tag {tag:#010x} {'OK' if tag == VENDOR_TAG else 'bad'}, version {version} (tail {version2}), items {item_num}{'' if ok else '  <- invalid'}")
        if ok and (best is None or version > best[0]):
            best = (version, blk, item_num)
    if not best:
        return
    _, blk, n = best
    data = blk[1024:]
    for i in range(n):
        iid, off, size, flag = struct.unpack_from("<HHHH", blk, 16 + 8 * i)
        val = data[off:off + size]
        name = VENDOR_IDS.get(iid, "?")
        if iid in (2, 3, 4) and size == 6:
            shown = ":".join(f"{b:02x}" for b in val)
        elif all(32 <= b < 127 for b in val.rstrip(b"\0")) and val.rstrip(b"\0"):
            shown = repr(val.rstrip(b"\0").decode())
        else:
            shown = f"{size} bytes, md5 {hashlib.md5(val).hexdigest()[:12]}"
        print(f"   item id {iid:3d} {name:16s} off {off:5d} size {size:4d}  {shown}")


def parse_parm(d):
    raw = d.read(PARM_LBA, 64)
    print(f"== LBA {PARM_LBA:#x} parameter block")
    if len(raw) < 8 or raw[:4] != b"PARM":
        print(f"   no PARM magic ({raw[:4]!r}); input too short or different layout")
        return None
    length = struct.unpack_from("<I", raw, 4)[0]
    text = raw[8:8 + length]
    crc_stored = struct.unpack_from("<I", raw, 8 + length)[0] if len(raw) >= 12 + length else None
    head_crc = zlib.crc32(raw[:942]) & 0xffffffff
    print(f"   length {length}, stored crc {crc_stored if crc_stored is None else hex(crc_stored)}, crc32(first 942 bytes) {head_crc:#010x} (known value 0x4a377dc5 on stock DHO units)")
    txt = text.decode("latin1")
    for line in txt.splitlines():
        if line.strip() and not line.startswith("CMDLINE"):
            print(f"   {line.strip()}")
    m = re.search(r"mtdparts=[^:]+:(\S+)", txt)
    if not m:
        print("   no mtdparts in CMDLINE")
        return None
    parts = []
    for p in m.group(1).split(","):
        mm = re.match(r"(-|0x[0-9a-fA-F]+)@(0x[0-9a-fA-F]+)\(([^):]+)(:grow)?\)", p)
        if not mm:
            continue
        size = None if mm.group(1) == "-" else int(mm.group(1), 16)
        off = int(mm.group(2), 16)
        parts.append((mm.group(3), PARM_LBA + off, size))
    print(f"   CMDLINE (other than mtdparts): {re.sub(r'mtdparts=\S+', '', txt.split('CMDLINE:', 1)[-1]).strip()[:200]}")
    return parts


def guess(b):
    if not b:
        return "(beyond input)"
    if b[:4] == b"KRNL":
        n = struct.unpack_from("<I", b, 4)[0]
        inner = b[8:12]
        what = "gzip ramdisk" if inner[:2] == b"\x1f\x8b" else ("arm64 Image" if b[8 + 0x38:8 + 0x3c] == b"ARMd" else inner.hex())
        return f"KRNL image, payload {n} bytes, {what}"
    if b[:6] == b"LOADER":
        return "Rockchip U-Boot (LOADER image)"
    if b[:4] == b"BL3X":
        return "Rockchip trust image (TF-A BL31 / OP-TEE)"
    if b[:4] == b"RSCE":
        return "RSCE resource (dtb + logos)"
    if b[:8] == b"ANDROID!":
        return "Android boot image"
    if b[:13] == b"boot-recovery" or b[:11] == b"boot-normal":
        return f"BCB command {b.split(b'\0')[0].decode()!r}"
    if len(b) > 0x43a and b[0x438:0x43a] == b"\x53\xef":
        return "ext4 filesystem"
    if b.count(0) == len(b):
        return "zero (first 4 KiB)"
    return "data " + b[:8].hex(" ")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("input")
    ap.add_argument("--extract")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--compare")
    a = ap.parse_args()
    d = Disk(a.input)
    if d.size:
        print(f"# {a.input}: {d.size} bytes = {d.size // SECTOR} sectors ({d.size / 1e9:.2f} GB)")
    show_head(d)
    show_vendor(d)
    parts = parse_parm(d)
    if not parts:
        return
    total = d.size // SECTOR if d.size else None
    print("== partitions (LBA absolute)")
    print(f"   {'name':14s} {'start':>10s} {'sectors':>10s} {'MiB':>9s}  content")
    for name, start, size in parts:
        sz = size if size is not None else ((total - start) if total else None)
        head = d.read(start, 8) if name != "misc" else d.read(start, 8)
        mib = f"{sz * SECTOR / 1048576:9.1f}" if sz else "     grow"
        print(f"   {name:14s} {start:10d} {sz if sz else '-':>10} {mib}  {guess(head)}")
        big = name in ("userdata", "system", "rigol", "cache", "metadata", "backup") or (sz or 0) > 256 * 2048
        if a.extract and (a.all or not big) and sz:
            os.makedirs(a.extract, exist_ok=True)
            with open(os.path.join(a.extract, name + ".bin"), "wb") as o:
                done = 0
                while done < sz:
                    n = min(8192, sz - done)
                    o.write(d.read(start + done, n))
                    done += n
        if a.compare and sz and os.path.exists(os.path.join(a.compare, name + ".bin")):
            ref = open(os.path.join(a.compare, name + ".bin"), "rb").read()
            cur = d.read(start, (len(ref) + SECTOR - 1) // SECTOR)[:len(ref)]
            same = hashlib.md5(ref).digest() == hashlib.md5(cur).digest()
            print(f"   {'':14s} compare with {name}.bin ({len(ref)} bytes): {'identical' if same else 'DIFFERENT'}")


if __name__ == "__main__":
    main()
