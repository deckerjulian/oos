#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""oos-bootimg.py: build, inspect and unpack the boot image that the stock Rockchip loader of the Rigol DHO800/DHO900
accepts in the `recovery` partition. Pure standard library, touches no device.

Format (see docs/boot.md):
  header page (2048):  "ANDROID!", kernel_size, kernel_addr 0x00280000, ramdisk_size, ramdisk_addr 0x04BF0000,
                       second_size, second_addr 0, tags_addr 0, page_size 2048, header_version 0, os_version 0,
                       name[16], cmdline[512], id[32], extra_cmdline[1024]
  kernel   uncompressed arm64 Image (text_offset 0), entered at 0x00280000
  ramdisk  gzip newc cpio, at 0x04BF0000
  second   Rockchip RSCE container with "rk-kernel.dtb"
  id       SHA-1(kernel, le32 len, ramdisk, le32 len, second, le32 len, header[32:576]), zero-padded to 32 bytes
           The loader refuses an image whose id is wrong. It hashes with the RK3399 crypto engine, so every
           section length must be a multiple of 4 (the ramdisk is padded here; kernel and RSCE already are).
  The loader builds the kernel command line from the DTB's /chosen/bootargs (not from the header field).

  oos-bootimg.py build  --kernel Image --dtb board.dtb [--ramdisk initrd.cpio.gz] [--name oos-dho900] --out oos.img
  oos-bootimg.py info   oos.img                 sections, addresses, id check, RSCE entries
  oos-bootimg.py unpack oos.img DIR             kernel, ramdisk, rk-kernel.dtb, cmdline.txt
  oos-bootimg.py mini-ramdisk OUT               a ramdisk with only /dev, /dev/console, /root (kernel mounts root=)
"""
import argparse, gzip, hashlib, os, struct, sys

PAGE = 2048
KERNEL_ADDR = 0x00280000
KERNEL_LIMIT = 0x00200000 + 0x3800000
RAMDISK_ADDR = 0x04BF0000
RAMDISK_SECOND_LIMIT = 0x3000000
RECOVERY_BYTES = 0x20000 * 512
ID_OFFSET = 8 + 10 * 4 + 16 + 512  # 576
HDR = struct.Struct("<8s10I16s512s32s1024s")


def pad(b, n=PAGE):
    return b + bytes(-len(b) % n)


# ---- Rockchip resource container ----------------------------------------------------------------------------
def rsce(entries):
    blk = 512
    hdr = bytearray(blk)
    hdr[0:4] = b"RSCE"
    hdr[8:12] = bytes((1, 1, 1, 0))          # header blocks, table offset (blocks), blocks per entry, pad
    struct.pack_into("<I", hdr, 12, len(entries))
    table, body = b"", b""
    cur = 1 + len(entries)
    for name, data in entries:
        row = bytearray(blk)
        row[0:4] = b"ENTR"
        row[4:4 + len(name)] = name.encode()
        struct.pack_into("<II", row, 260, cur, len(data))
        table += bytes(row)
        body += pad(data, blk)
        cur += (len(data) + blk - 1) // blk
    return bytes(hdr) + table + body


def rsce_entries(blob):
    if blob[:4] != b"RSCE":
        raise ValueError(f"second is not RSCE ({blob[:4]!r})")
    toff, eblk = blob[9], blob[10]
    n = struct.unpack_from("<I", blob, 12)[0]
    out = []
    for i in range(n):
        row = blob[(toff + i * eblk) * 512:]
        name = row[4:260].split(b"\0")[0].decode()
        first, size = struct.unpack_from("<II", row, 260)
        out.append((name, blob[first * 512:first * 512 + size]))
    return out


# ---- minimal initramfs (newc) -------------------------------------------------------------------------------
def cpio(members):
    out = bytearray()

    def put(ino, name, mode, data=b"", nlink=1, rdev=(0, 0)):
        raw = name.encode() + b"\0"
        f = (ino, mode, 0, 0, nlink, 0, len(data), 0, 0, rdev[0], rdev[1], len(raw), 0)
        out.extend(b"070701" + b"".join(b"%08x" % x for x in f) + raw)
        out.extend(bytes(-len(out) % 4))
        out.extend(data)
        out.extend(bytes(-len(out) % 4))

    for ino, (name, mode, payload) in enumerate(members, 1):
        kind = mode & 0o170000
        if kind == 0o040000:
            put(ino, name, mode, nlink=2)
        elif kind in (0o020000, 0o060000):
            put(ino, name, mode, rdev=payload)
        elif kind == 0o120000:
            put(ino, name, mode, payload.encode())
        else:
            put(ino, name, mode, payload)
    put(0, "TRAILER!!!", 0)
    return bytes(out)


def mini_ramdisk():
    blob = gzip.compress(cpio([("dev", 0o040755, b""), ("dev/console", 0o020600, (5, 1)), ("root", 0o040700, b"")]),
                         compresslevel=9, mtime=0)
    return blob + bytes(-len(blob) % 4)


# ---- the image ----------------------------------------------------------------------------------------------
def image_id(header, kernel, ramdisk, second):
    h = hashlib.sha1()
    for part in (kernel, ramdisk, second):
        h.update(part)
        h.update(struct.pack("<I", len(part)))
    h.update(header[32:ID_OFFSET])
    return h.digest().ljust(32, b"\0")


def check_kernel(img, strict=True):
    problems = []
    if img[:2] == b"\x1f\x8b":
        problems.append("gzip-compressed; the loader needs the raw Image")
    if img[0x38:0x3c] != b"ARM\x64":
        problems.append("no arm64 Image magic at 0x38")
    text_offset, image_size = struct.unpack_from("<QQ", img, 8)
    if text_offset != 0:
        problems.append(f"text_offset {text_offset:#x} (a 6.x Image has 0; 4.4 kernels use 0x80000)")
    end = KERNEL_ADDR + max(image_size, len(img))
    if end > KERNEL_LIMIT:
        problems.append(f"Image + BSS end at {end:#x}, past {KERNEL_LIMIT:#x}")
    if len(img) % 4:
        problems.append("length not a multiple of 4")
    if problems and strict:
        raise SystemExit("kernel: " + "; ".join(problems))
    return problems


def dtb_bootargs(dtb):
    """/chosen/bootargs from a flattened device tree (minimal FDT walker)."""
    if dtb[:4] != b"\xd0\x0d\xfe\xed":
        raise SystemExit("dtb: no FDT magic")
    off_struct, off_strings = struct.unpack_from(">I", dtb, 8)[0], struct.unpack_from(">I", dtb, 12)[0]
    p, path = off_struct, []
    while True:
        tok = struct.unpack_from(">I", dtb, p)[0]
        p += 4
        if tok == 1:
            end = dtb.index(b"\0", p)
            path.append(dtb[p:end].decode())
            p = (end + 4) & ~3
        elif tok == 2:
            path.pop()
        elif tok == 3:
            ln, nameoff = struct.unpack_from(">II", dtb, p)
            p += 8
            name = dtb[off_strings + nameoff:dtb.index(b"\0", off_strings + nameoff)].decode()
            val = dtb[p:p + ln]
            p = (p + ln + 3) & ~3
            if path[1:] == ["chosen"] and name == "bootargs":
                return val.rstrip(b"\0").decode()
        elif tok == 4:
            continue
        elif tok == 9:
            return ""


def build(kernel, dtb, ramdisk, name="oos-dho900", cmdline=None):
    check_kernel(kernel)
    # The loader hashes with the RK3399 crypto engine, which works on 32-bit words: a ramdisk whose length is not a
    # multiple of 4 gives a wrong hash and the image is refused (tested 2026-11-02). Zeros after a gzip initramfs are
    # ignored by the kernel.
    ramdisk = ramdisk + bytes(-len(ramdisk) % 4)
    bootargs = dtb_bootargs(dtb)
    if not bootargs:
        print("warning: the DTB has no /chosen/bootargs; the loader builds the kernel command line from it", file=sys.stderr)
    second = rsce([("rk-kernel.dtb", dtb)])
    if len(ramdisk) + len(second) > RAMDISK_SECOND_LIMIT:
        raise SystemExit("ramdisk + DTB container too large for the loader")
    cl = (cmdline if cmdline is not None else bootargs).encode()[:511]
    header = bytearray(HDR.pack(b"ANDROID!", len(kernel), KERNEL_ADDR, len(ramdisk), RAMDISK_ADDR, len(second), 0,
                                0, PAGE, 0, 0, name.encode()[:15], cl, bytes(32), bytes(1024)))
    header[ID_OFFSET:ID_OFFSET + 32] = image_id(bytes(header), kernel, ramdisk, second)
    blob = pad(bytes(header)) + pad(kernel) + pad(ramdisk) + pad(second)
    if len(blob) > RECOVERY_BYTES:
        raise SystemExit(f"image {len(blob)} bytes does not fit `recovery` ({RECOVERY_BYTES})")
    return blob


def parts(blob):
    if blob[:8] != b"ANDROID!":
        raise SystemExit("not an Android boot image")
    f = struct.unpack_from("<10I", blob, 8)
    ks, rs, ss, page = f[0], f[2], f[4], f[7]
    k0 = page
    r0 = k0 + (ks + page - 1) // page * page
    s0 = r0 + (rs + page - 1) // page * page
    return f, blob[k0:k0 + ks], blob[r0:r0 + rs], blob[s0:s0 + ss]


def info(path):
    blob = open(path, "rb").read()
    f, k, r, s = parts(blob)
    idok = blob[ID_OFFSET:ID_OFFSET + 32] == image_id(blob[:PAGE], k, r, s)
    print(f"{path}: {len(blob)} bytes (recovery holds {RECOVERY_BYTES})")
    print(f"  kernel  {f[0]:9d} bytes @ {f[1]:#010x}  {'; '.join(check_kernel(k, strict=False)) or 'arm64 Image ok'}")
    kind = "gzip" if r[:2] == b"\x1f\x8b" else repr(r[:4])
    name = blob[48:64].split(b"\0")[0].decode()
    cmdline = blob[64:576].split(b"\0")[0].decode()
    print(f"  ramdisk {f[2]:9d} bytes @ {f[3]:#010x}  {kind}")
    print(f"  second  {f[4]:9d} bytes @ {f[5]:#010x}")
    print(f"  page {f[7]}, header_version {f[8]}, name {name!r}")
    print(f"  cmdline field: {cmdline!r}")
    print(f"  id {blob[ID_OFFSET:ID_OFFSET + 20].hex()}  {'OK' if idok else 'WRONG (the loader would refuse this image)'}")
    for name, data in rsce_entries(s):
        extra = f", bootargs {dtb_bootargs(data)!r}" if name.endswith(".dtb") else ""
        print(f"  RSCE {name}: {len(data)} bytes{extra}")
    return idok


def main():
    ap = argparse.ArgumentParser()
    sp = ap.add_subparsers(dest="cmd", required=True)
    b = sp.add_parser("build")
    b.add_argument("--kernel", required=True)
    b.add_argument("--dtb", required=True)
    b.add_argument("--ramdisk")
    b.add_argument("--name", default="oos-dho900")
    b.add_argument("--cmdline")
    b.add_argument("--out", required=True)
    sp.add_parser("info").add_argument("image")
    u = sp.add_parser("unpack")
    u.add_argument("image")
    u.add_argument("dir")
    sp.add_parser("mini-ramdisk").add_argument("out")
    a = ap.parse_args()
    if a.cmd == "build":
        rd = open(a.ramdisk, "rb").read() if a.ramdisk else mini_ramdisk()
        blob = build(open(a.kernel, "rb").read(), open(a.dtb, "rb").read(), rd, a.name, a.cmdline)
        open(a.out, "wb").write(blob)
        info(a.out)
    elif a.cmd == "info":
        sys.exit(0 if info(a.image) else 1)
    elif a.cmd == "unpack":
        blob = open(a.image, "rb").read()
        _, k, r, s = parts(blob)
        os.makedirs(a.dir, exist_ok=True)
        open(os.path.join(a.dir, "kernel"), "wb").write(k)
        open(os.path.join(a.dir, "ramdisk"), "wb").write(r)
        for name, data in rsce_entries(s):
            open(os.path.join(a.dir, name), "wb").write(data)
        open(os.path.join(a.dir, "cmdline.txt"), "wb").write(blob[64:576].split(b"\0")[0] + b"\n")
    elif a.cmd == "mini-ramdisk":
        open(a.out, "wb").write(mini_ramdisk())


if __name__ == "__main__":
    main()
