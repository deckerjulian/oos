#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build-fsck.sh - a static e2fsck for the image, in Docker, into rootfs/out/. boot/build-dev-image.sh runs it when it is
# missing or out of date (deps.sh). The scope is switched off at the mains, so /oos is checked before every mount
# (oos-mount-data).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
. "$here/../tools/lib/docker.sh"
ver=$(dep_ver e2fsprogs)
dep_fetch e2fsprogs "$out" >/dev/null
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" "$OOS_ALPINE" sh -euc '
	apk add --no-cache build-base linux-headers pkgconf >/dev/null
	cd /tmp && tar -xJf /out/e2fsprogs-$VER.tar.xz && cd e2fsprogs-$VER
	./configure --disable-nls --disable-uuidd --disable-fuse2fs --disable-defrag --disable-imager --disable-resizer CFLAGS=-Os >/tmp/conf.log 2>&1 || { tail -20 /tmp/conf.log; exit 1; }
	make -j"$(nproc)" libs >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	make -C e2fsck -j"$(nproc)" e2fsck.static >>/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip e2fsck/e2fsck.static
	cp e2fsck/e2fsck.static /out/e2fsck
	cp NOTICE /out/e2fsck.COPYING
	chown "$OUT_OWNER" /out/*
'
dep_stamp "$out/e2fsck" "$here/build-fsck.sh" e2fsprogs
ls -la "$out/e2fsck"
