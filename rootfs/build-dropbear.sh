#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# build-dropbear.sh - a static Dropbear SSH server for the image (docs/network.md 7), in Docker, into rootfs/out/.
# boot/unit-hook.sh runs it when it is missing. One multi-call binary with dropbear, dropbearkey and scp (the server
# side of `scp -O`); no zlib, no sftp-server, no X11 or agent forwarding. The SHA-512 of the tarball is the one Alpine
# (aports main/dropbear) builds its package from; the SHA-256 below was taken from that same verified file.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
ver=2026.94
sum=e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81
mkdir -p "$out"
tarball="$out/dropbear-$ver.tar.bz2"
[ -f "$tarball" ] || curl -fsSL -o "$tarball" "https://matt.ucc.asn.au/dropbear/releases/dropbear-$ver.tar.bz2"
echo "$sum  $tarball" | { shasum -a 256 -c - 2>/dev/null || sha256sum -c -; } >/dev/null || { echo "wrong checksum: $tarball" >&2; rm -f "$tarball"; exit 1; }
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers >/dev/null
	cd /tmp && tar -xjf /out/dropbear-$VER.tar.bz2 && cd dropbear-$VER
	cat > localoptions.h <<-EOF
	#define DROPBEAR_SFTPSERVER 0
	#define DROPBEAR_SVR_AGENTFWD 0
	#define DROPBEAR_X11FWD 0
	#define DO_MOTD 0
	#define DROPBEAR_PIDFILE "/run/dropbear.pid"
	#define DEFAULT_PATH "/bin:/sbin"
	#define DEFAULT_ROOT_PATH "/bin:/sbin"
	EOF
	./configure --enable-static --disable-zlib --disable-lastlog --disable-utmp --disable-utmpx --disable-wtmp \
		--disable-wtmpx --disable-pututline --disable-pututxline >/tmp/configure.log 2>&1 || { tail -30 /tmp/configure.log; exit 1; }
	make -j"$(nproc)" PROGRAMS="dropbear dropbearkey scp" MULTI=1 STATIC=1 >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip -o /out/dropbearmulti dropbearmulti
	cp LICENSE /out/dropbear.COPYING
	chown "$OUT_OWNER" /out/dropbearmulti /out/dropbear.COPYING
'
file "$out/dropbearmulti" 2>/dev/null || true
ls -la "$out/dropbearmulti"
