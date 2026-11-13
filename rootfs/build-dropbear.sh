#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# build-dropbear.sh - a static Dropbear SSH server for the image (docs/network.md 7), in Docker, into rootfs/out/.
# boot/unit-hook.sh runs it when it is missing or out of date (deps.sh). One multi-call binary with dropbear,
# dropbearkey and scp (the server side of `scp -O`); the SFTP subsystem runs /bin/sftp-server (build-sftp-server.sh),
# which modern `scp` and `sftp` use. No zlib, no X11 or agent forwarding. Version and SHA-256: rootfs/deps.lock (the
# SHA-512 of 2026.94 is the one Alpine's aports main/dropbear builds from; the SHA-256 was taken from that same file).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
ver=$(dep_ver dropbear)
dep_fetch dropbear "$out" >/dev/null
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers >/dev/null
	cd /tmp && tar -xjf /out/dropbear-$VER.tar.bz2 && cd dropbear-$VER
	cat > localoptions.h <<-EOF
	#define DROPBEAR_SFTPSERVER 1
	#define SFTPSERVER_PATH "/bin/sftp-server"
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
dep_stamp "$out/dropbearmulti" "$0" dropbear
file "$out/dropbearmulti" 2>/dev/null || true
ls -la "$out/dropbearmulti"
