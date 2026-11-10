#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""image-manifest.py: what an image build put into the image, one line per file, to compare two builds.

    tools/image-manifest.py kernel/out/base/Image boot/out/oos-dho900.dtb boot/out/initramfs.cpio.gz

Prints `sha256  name` for the kernel, the DTB and every regular file of the initramfs (newc cpio, gzip), `-> target`
for its links and `dev`/`dir` for the rest, with the mode, sorted by name. `etc/oos-build` is left out: it holds the
build time. Two builds of the same tree have the same manifest when they are bit for bit the same inside.
"""
import gzip, hashlib, os, sys

SKIP = {"etc/oos-build"}


def cpio_entries(data):
    o = 0
    while o + 110 <= len(data):
        h = data[o:o + 110]
        if h[:6] not in (b"070701", b"070702"):
            sys.exit(f"not a newc cpio at offset {o}")
        f = [int(h[6 + 8 * i:14 + 8 * i], 16) for i in range(13)]
        mode, size, namesize = f[1], f[6], f[11]
        name = data[o + 110:o + 110 + namesize - 1].decode()
        o = (o + 110 + namesize + 3) & ~3
        body = data[o:o + size]
        o = (o + size + 3) & ~3
        if name == "TRAILER!!!":
            return
        yield name, mode, body


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    image, dtb, initramfs = sys.argv[1:]
    lines = []
    for label, path in (("kernel/Image", image), ("kernel/" + os.path.basename(dtb), dtb)):
        lines.append(f"{hashlib.sha256(open(path, 'rb').read()).hexdigest()}  {label}")
    for name, mode, body in cpio_entries(gzip.open(initramfs).read()):
        name = name.removeprefix("./")
        if name in SKIP or name == ".":
            continue
        kind = mode & 0o170000
        perm = f"{mode & 0o7777:04o}"
        if kind == 0o100000:
            lines.append(f"{hashlib.sha256(body).hexdigest()}  {perm}  initramfs/{name}")
        elif kind == 0o120000:
            lines.append(f"-> {body.decode()}  {perm}  initramfs/{name}")
        else:
            lines.append(f"{'dir' if kind == 0o040000 else 'dev'}  {perm}  initramfs/{name}")
    print("\n".join(sorted(lines, key=lambda l: l.rsplit("  ", 1)[-1])))


if __name__ == "__main__":
    main()
