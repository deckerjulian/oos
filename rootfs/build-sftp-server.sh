#!/bin/sh
# Copyright (C) 2026 OOS contributors
# SPDX-License-Identifier: GPL-2.0-or-later
# build-sftp-server.sh - OpenSSH's sftp-server alone, static, for the image (docs/network.md 7), in Docker, into
# rootfs/out/. Dropbear runs it as /bin/sftp-server for the SFTP subsystem (build-dropbear.sh): `sftp`, and `scp`
# without -O, which OpenSSH 9.0 and later use by default. Built without OpenSSL and zlib: the program does file
# transfer over the channel Dropbear already encrypted. boot/unit-hook.sh runs it when it is missing or out of date.
# Version and SHA-256: rootfs/deps.lock (10.6p2: checked against the base64 SHA256 in OpenSSH's release notes).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
. "$here/deps.sh"
ver=$(dep_ver openssh)
dep_fetch openssh "$out" >/dev/null
# arm64 containers: on an x86-64 host through QEMU (boot/build-dev-image.sh says how); the results belong to the caller
docker run --rm --platform linux/arm64 -e OUT_OWNER="$(id -u):$(id -g)" -e VER="$ver" -v "$out:/out" alpine:3.22 sh -euc '
	apk add --no-cache build-base linux-headers >/dev/null
	cd /tmp && tar -xzf /out/openssh-$VER.tar.gz && cd openssh-$VER
	./configure --without-openssl --without-zlib --without-pam --without-selinux --without-kerberos5 \
		--without-libedit --without-security-key-builtin --disable-lastlog --disable-utmp --disable-utmpx \
		--disable-wtmp --disable-wtmpx LDFLAGS=-static >/tmp/configure.log 2>&1 || { tail -30 /tmp/configure.log; exit 1; }
	make -j"$(nproc)" sftp-server >/tmp/build.log 2>&1 || { tail -30 /tmp/build.log; exit 1; }
	strip -o /out/sftp-server sftp-server
	cp LICENCE /out/sftp-server.COPYING
	chown "$OUT_OWNER" /out/sftp-server /out/sftp-server.COPYING
'
dep_stamp "$out/sftp-server" "$0" openssh
file "$out/sftp-server" 2>/dev/null || true
ls -la "$out/sftp-server"
