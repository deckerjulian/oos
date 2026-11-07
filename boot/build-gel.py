#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""build-gel.py: the update file that installs OOS from the running stock system, without opening the scope
(docs/install-usb.md).

    boot/build-gel.py        -> boot/out/DHO800_DHO900_OOS_Install.GEL

From boot/out/oos-dev.oosupd and oos-update, as boot/build-dev-image.sh leaves them. The stock update takes any gzip tar
whose name ends in .GEL and holds DHO800_DHO900, unpacks it and runs its shell/do_extract.sh.
"""
import gzip, io, os, sys, tarfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
PKG = os.path.join(HERE, "out", "oos-dev.oosupd")
TOOL = os.path.join(ROOT, "fpgad", "target", "aarch64-unknown-linux-musl", "release", "oos-update")
OUT = os.path.join(HERE, "out", "DHO800_DHO900_OOS_Install.GEL")


def add(t, name, path, mode):
    data = open(path, "rb").read()
    i = tarfile.TarInfo(name)
    i.size, i.mode, i.mtime = len(data), mode, int(os.path.getmtime(PKG))
    t.addfile(i, io.BytesIO(data))


def folder(t, name):
    i = tarfile.TarInfo(name)
    i.type, i.mode, i.mtime = tarfile.DIRTYPE, 0o755, int(os.path.getmtime(PKG))
    t.addfile(i)


def main():
    for p in (PKG, TOOL):
        if not os.path.exists(p):
            sys.exit(f"missing {p}: run boot/build-dev-image.sh first")
    if os.path.getmtime(TOOL) > os.path.getmtime(PKG) + 3600 or os.path.getmtime(PKG) > os.path.getmtime(TOOL) + 3600:
        print("note: oos-update and the package are more than an hour apart")
    b = io.BytesIO()
    with gzip.GzipFile(fileobj=b, mode="wb", mtime=0) as z:
        with tarfile.open(fileobj=z, mode="w", format=tarfile.GNU_FORMAT) as t:
            folder(t, "shell")
            add(t, "shell/do_extract.sh", os.path.join(HERE, "gel", "do_extract.sh"), 0o755)
            folder(t, "oos")
            add(t, "oos/oos-update", TOOL, 0o755)
            add(t, "oos/oos.oosupd", PKG, 0o644)
    open(OUT, "wb").write(b.getvalue())
    print(f"{OUT}: {len(b.getvalue())} bytes")


if __name__ == "__main__":
    main()
