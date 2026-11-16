#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# build-kexec.sh - a static kexec (kexec-tools) for the image, in Docker, into rootfs/out/. boot/build-dev-image.sh runs
# it when it is missing or out of date (deps.sh). oos-update starts an update whose device tree differs from the
# running one with it: the kexec_file_load call it uses otherwise gives the new kernel the running tree, kexec_load
# (`kexec -c --dtb`) takes the new one (docs/update.md, "Trial start by kexec").
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
# the checksum in rootfs/deps.lock is the one of kernel.org's pub/linux/utils/kernel/kexec/sha256sums.asc
. "$here/deps.sh"
ver=$(dep_ver kexec-tools)
dep_fetch kexec-tools "$out" >/dev/null
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers >/dev/null
	cd /tmp && tar -xJf /out/kexec-tools-$VER.tar.xz && cd kexec-tools-$VER
	./configure --without-zlib --without-lzma --without-xen CFLAGS=-Os LDFLAGS=-static >/tmp/conf.log 2>&1 || { tail -20 /tmp/conf.log; exit 1; }
	make -j"$(nproc)" >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip build/sbin/kexec
	cp build/sbin/kexec /out/kexec
	cp COPYING /out/kexec.COPYING
	chown "$OUT_OWNER" /out/kexec /out/kexec.COPYING /out/kexec-tools-$VER.tar.xz
'
dep_stamp "$out/kexec" "$here/build-kexec.sh" kexec-tools
ls -la "$out/kexec"
