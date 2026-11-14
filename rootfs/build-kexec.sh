#!/bin/sh
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# build-kexec.sh - a static kexec (kexec-tools) for the image, in Docker, into rootfs/out/. boot/build-dev-image.sh runs
# it when it is missing. oos-update starts an update whose device tree differs from the running one with it: the
# kexec_file_load call it uses otherwise gives the new kernel the running tree, kexec_load (`kexec -c --dtb`) takes the
# new one (docs/update.md, "Trial start by kexec").
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
ver=2.0.32
# kernel.org, pub/linux/utils/kernel/kexec/sha256sums.asc
sum=8f81422a5fd2362cf6cb001b511e535565ed0f32c2f4451fb5eb68fed6710a5d
mkdir -p "$out"
tarball="$out/kexec-tools-$ver.tar.xz"
[ -f "$tarball" ] || curl -fsSL -o "$tarball" "https://mirrors.edge.kernel.org/pub/linux/utils/kernel/kexec/kexec-tools-$ver.tar.xz"
echo "$sum  $tarball" | { shasum -a 256 -c - 2>/dev/null || sha256sum -c -; } >/dev/null || { echo "wrong checksum: $tarball" >&2; rm -f "$tarball"; exit 1; }
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
ls -la "$out/kexec"
